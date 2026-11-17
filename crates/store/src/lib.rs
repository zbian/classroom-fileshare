//! 课堂共享存储层：SQLite 索引 + 文件仓库。
//!
//! 覆盖语义（PRD 5.7 / 7.3）：作用域键 `owner|audience|名称` 命中时执行
//! `.part → SHA-256 → 原子替换 → 旧版转唯一 .bak → version+1` 的幂等 upsert；
//! 文件名经 [`classroom_protocol::sanitize_file_name`] 净化（剥路径成分，
//! 含绝对路径与 `..` 前缀——净化后只剩文件名、写入 owner 子目录，不可能逃逸），
//! 另加 Windows 保留设备名与风险扩展名拦截（PRD 9.4 / 5.3）。

pub mod files;

use std::path::{Component, Path, PathBuf};

use classroom_protocol::{
    merge, sanitize_file_name, validate_text, Audience, Event, ItemKind, MergeDecision, NameReject,
    Role, ShareItem, SyncState, TextReject, MAX_FILE_SIZE,
};
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub use files::sha256_file;

/// 索引 schema 版本（PRAGMA user_version）。
const SCHEMA_VERSION: i64 = 1;

const ITEM_COLS: &str = "id, kind, owner_id, owner_role, audience, title, size, sha256, \
                         rel_path, created_at, updated_at, version, deleted, sync_state";

/// 存储层错误。
#[derive(Debug, Error)]
pub enum StoreError {
    /// 文件系统 IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    /// SQLite 错误。
    #[error("数据库错误: {0}")]
    Sql(#[from] rusqlite::Error),
    /// 文件名净化失败。
    #[error("文件名非法: {0}")]
    Name(#[from] NameReject),
    /// 文本校验失败。
    #[error("文本非法: {0}")]
    Text(#[from] TextReject),
    /// owner_id 含路径成分或非法字符，拒绝作为目录名。
    #[error("发送者标识非法: {0}")]
    BadOwner(String),
    /// 高风险扩展名（PRD 5.3）。
    #[error("高风险文件类型: {0}")]
    RiskyExt(String),
    /// 单文件超过 100 MB 上限（PRD 5.3/9.4、EX-04；E10）。
    /// 文案为 EX-04 逐字；携带实际字节数供调用方展示/日志。
    #[error("单文件不能超过 100 MB")]
    FileTooLarge(u64),
    /// 临时文件 SHA-256 与期望不符（PRD EX-07）。
    #[error("哈希校验失败: 期望 {expected}，实际 {actual}")]
    HashMismatch { expected: String, actual: String },
    /// 条目不存在。
    #[error("条目不存在: {0}")]
    NotFound(String),
    /// 索引 schema 版本过新，程序需要升级。
    #[error("不支持的 schema 版本: {0}")]
    UnsupportedSchema(i64),
}

/// 存储层 Result 别名。
pub type Result<T> = std::result::Result<T, StoreError>;

/// put 结果：新建或同名覆盖（PRD 5.7）。
#[derive(Debug, PartialEq)]
pub enum PutOutcome {
    /// 作用域键未命中：新建，version = 1。
    Created(ShareItem),
    /// 作用域键命中：覆盖，返回最新条目与被覆盖的旧 version。
    Overwritten {
        /// 覆盖后的最新条目（version 已 +1）。
        latest: ShareItem,
        /// 被覆盖前的 version。
        old_version: u64,
    },
}

/// 列表过滤条件（全部为可选项）。
/// 接口修订 #6③：补 Serialize/Deserialize/PartialEq（作为 ipc 层
/// `GuiRequest::List` 载荷跨命名管道传输所需）。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListFilter {
    /// 按发送者过滤。
    pub owner: Option<String>,
    /// 按接收范围过滤。
    pub audience: Option<Audience>,
    /// 按类型过滤。
    pub kind: Option<ItemKind>,
    /// 按标题关键字过滤（大小写不敏感的包含匹配）。
    pub keyword: Option<String>,
    /// 是否包含软删除（墓碑）条目，默认不含。
    #[serde(default)]
    pub include_deleted: bool,
}

/// 判断是否为 Windows 保留设备名——复用 protocol 的实现（PR #28 检查点①）。
///
/// 这类名字即使带扩展名也会被系统解析为设备，落盘前必须拒绝（PRD 9.4）；
/// 单一清单放 protocol 层，避免 store 与 protocol 两份漂移。
pub use classroom_protocol::is_reserved_device_name;

// ---------- 枚举与字符串互转（入库/出库） ----------

fn kind_name(k: &ItemKind) -> &'static str {
    match k {
        ItemKind::Text => "Text",
        ItemKind::File => "File",
    }
}

fn role_name(r: &Role) -> &'static str {
    match r {
        Role::Teacher => "Teacher",
        Role::Student => "Student",
    }
}

fn audience_name(a: &Audience) -> &'static str {
    match a {
        Audience::All => "All",
        Audience::Teacher => "Teacher",
    }
}

/// 与 [`ShareItem::scope_key`] 相同的三元组字符串（owner|audience|title）。
fn scope_key_str(owner: &str, audience: &Audience, title: &str) -> String {
    let a = match audience {
        Audience::All => "all",
        Audience::Teacher => "teacher",
    };
    format!("{owner}|{a}|{title}")
}

fn sync_state_name(s: &SyncState) -> &'static str {
    match s {
        SyncState::Synced => "Synced",
        SyncState::PendingSend => "PendingSend",
        SyncState::PendingSync => "PendingSync",
        SyncState::DeleteDeferred => "DeleteDeferred",
        SyncState::RetryLocked => "RetryLocked",
        SyncState::Failed => "Failed",
    }
}

fn bad_enum(col: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        format!("列 {col} 的未知枚举值: {value}").into(),
    )
}

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<ShareItem> {
    let kind = match row.get::<_, String>(1)?.as_str() {
        "Text" => ItemKind::Text,
        "File" => ItemKind::File,
        v => return Err(bad_enum("kind", v)),
    };
    let owner_role = match row.get::<_, String>(3)?.as_str() {
        "Teacher" => Role::Teacher,
        "Student" => Role::Student,
        v => return Err(bad_enum("owner_role", v)),
    };
    let audience = match row.get::<_, String>(4)?.as_str() {
        "All" => Audience::All,
        "Teacher" => Audience::Teacher,
        v => return Err(bad_enum("audience", v)),
    };
    let sync_state = match row.get::<_, String>(13)?.as_str() {
        "Synced" => SyncState::Synced,
        "PendingSend" => SyncState::PendingSend,
        "PendingSync" => SyncState::PendingSync,
        "DeleteDeferred" => SyncState::DeleteDeferred,
        "RetryLocked" => SyncState::RetryLocked,
        "Failed" => SyncState::Failed,
        v => return Err(bad_enum("sync_state", v)),
    };
    Ok(ShareItem {
        id: row.get(0)?,
        kind,
        owner_id: row.get(2)?,
        owner_role,
        audience,
        title: row.get(5)?,
        size: row.get(6)?,
        sha256: row.get(7)?,
        rel_path: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        version: row.get(11)?,
        deleted: row.get::<_, i64>(12)? != 0,
        sync_state,
    })
}

/// owner_id 作为目录名的防御性校验（PRD 9.4：只写应用数据目录内、拒绝穿越）。
fn safe_component(raw: &str) -> Result<&str> {
    let bad = raw.is_empty()
        || raw == "."
        || raw == ".."
        || raw.chars().any(|c| {
            matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control()
        });
    if bad {
        return Err(StoreError::BadOwner(raw.to_string()));
    }
    Ok(raw)
}

/// 作用域键哈希短名：同作用域稳定、跨作用域几乎不撞、且必然是合法文件名。
fn scope_hash(owner: &str, audience: &Audience, title: &str) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(scope_key_str(owner, audience, title).as_bytes());
    files::to_hex(&d)[..12].to_string()
}

/// SQLite 索引 + 文件仓库。
///
/// 布局：`root/repo/<owner_id>/<净化后文件名>`（文件）、
/// `root/repo/<owner_id>/texts/t<hash>.txt`（文本正文）、备份 `*.bak` 同目录仅 1 份、
/// 索引 `root/index.db`。
pub struct Store {
    conn: Connection,
    root: PathBuf,
}

/// BUSY/LOCKED 类瞬时错误判定（open 重试依据）。
fn is_busy_like(e: &StoreError) -> bool {
    matches!(
        e,
        StoreError::Sql(rusqlite::Error::SqliteFailure(ffi_err, _))
            if ffi_err.code == rusqlite::ffi::ErrorCode::DatabaseBusy
                || ffi_err.code == rusqlite::ffi::ErrorCode::DatabaseLocked
    )
}

impl Store {
    /// 打开（或初始化）`root` 下的仓库与索引，执行 schema v1 幂等迁移。
    ///
    /// 对 BUSY/LOCKED 类瞬时错误重试 3 次 × 200ms（`journal_mode=WAL`
    /// 切换需短暂独占，busy_timeout 对 mode 切换不兜底；E7 同分支实测
    /// 全 workspace 并行跑批时单发）。
    pub fn open(root: &Path) -> Result<Self> {
        let mut last_err: Option<StoreError> = None;
        for attempt in 0..3 {
            match Self::open_once(root) {
                Ok(s) => return Ok(s),
                Err(e) if is_busy_like(&e) => {
                    last_err = Some(e);
                    tracing::warn!(attempt = attempt + 1, "store open 遇 busy，200ms 后重试");
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| StoreError::UnsupportedSchema(-1)))
    }

    fn open_once(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root.join("repo"))?;
        let conn = Connection::open(root.join("index.db"))?;
        // WAL（接口/行为修订 #8③，仅 PRAGMA 无签名变化）：并发读写解耦——
        // 服务内 handler store 与核心 store 两个连接长期并存，WAL 使 busy
        // 窗口趋近于零（CI 慢速卷上 500ms 超时被建表写锁撞穿，run 82）。
        // journal_mode 需以查询方式设置（返回生效模式）；持久化在 db 文件，
        // 重复打开幂等。注意落盘产生 index.db-wal/-shm 同目录文件（数据
        // 保留与卸载语义不变，PRD 9.5）。
        let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        tracing::debug!(%mode, "journal_mode");
        // busy_timeout 提到 5000ms：慢速卷（CI）上写锁等待兜底
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v > SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchema(v));
        }
        if v < SCHEMA_VERSION {
            conn.execute_batch(include_str!("schema.sql"))?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        tracing::debug!(root = %root.display(), "store opened");
        Ok(Self {
            conn,
            root: root.to_path_buf(),
        })
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    /// 查询 PRAGMA（测试/诊断用；doc(hidden) 不入公开文档）。
    #[doc(hidden)]
    pub fn pragma_query(&self, name: &str) -> Result<String> {
        self.conn
            .query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))
            .map_err(Into::into)
    }

    /// 相对 `root/repo/` 的正斜杠路径（入库 rel_path）。
    fn rel_of(&self, path: &Path) -> String {
        path.strip_prefix(self.repo())
            .expect("仓库内路径")
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn find_scope(
        &self,
        owner: &str,
        audience: &Audience,
        title: &str,
    ) -> Result<Option<ShareItem>> {
        let sql = format!(
            "SELECT {ITEM_COLS} FROM share_items \
                           WHERE owner_id = ?1 AND audience = ?2 AND title = ?3 AND deleted = 0"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        match stmt.query_row(params![owner, audience_name(audience), title], row_to_item) {
            Ok(item) => Ok(Some(item)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 按 id 写入（主键 REPLACE：覆盖场景沿用旧行 id）。
    fn upsert_item(&self, item: &ShareItem) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO share_items \
             (id, kind, owner_id, owner_role, audience, title, size, sha256, rel_path, \
              created_at, updated_at, version, deleted, sync_state) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                item.id,
                kind_name(&item.kind),
                item.owner_id,
                role_name(&item.owner_role),
                audience_name(&item.audience),
                item.title,
                item.size,
                item.sha256,
                item.rel_path,
                item.created_at,
                item.updated_at,
                item.version,
                item.deleted as i64,
                sync_state_name(&item.sync_state),
            ],
        )?;
        Ok(())
    }

    /// 存入或覆盖一条文本：正文原子落盘，作用域键命中时旧版转唯一 `.bak`。
    pub fn put_text(
        &self,
        owner_id: &str,
        role: Role,
        audience: Audience,
        title: &str,
        body: &str,
        now: i64,
    ) -> Result<PutOutcome> {
        let owner = safe_component(owner_id)?;
        validate_text(title, body)?;
        let old = self.find_scope(owner, &audience, title)?;

        let texts_dir = self.repo().join(owner).join("texts");
        std::fs::create_dir_all(&texts_dir)?;
        let h = scope_hash(owner, &audience, title);
        let final_path = texts_dir.join(format!("t{h}.txt"));
        let bak_path = texts_dir.join(format!("t{h}.txt.bak"));
        let part_path = texts_dir.join(format!("t{h}.txt.part"));

        // .part → （旧版转 .bak）→ 原子替换（PRD 7.3）
        std::fs::write(&part_path, body)?;
        let rename = |src: &Path, dst: &Path| std::fs::rename(src, dst);
        if final_path.exists() {
            rename(&final_path, &bak_path)?;
        }
        rename(&part_path, &final_path)?;

        let item = match &old {
            None => ShareItem {
                id: Uuid::new_v4().to_string(),
                kind: ItemKind::Text,
                owner_id: owner.to_string(),
                owner_role: role,
                audience,
                title: title.to_string(),
                size: body.chars().count() as u64,
                sha256: None,
                rel_path: Some(self.rel_of(&final_path)),
                created_at: now,
                updated_at: now,
                version: 1,
                deleted: false,
                sync_state: SyncState::Synced,
            },
            Some(prev) => ShareItem {
                title: title.to_string(),
                size: body.chars().count() as u64,
                sha256: None,
                rel_path: Some(self.rel_of(&final_path)),
                updated_at: now,
                version: prev.version + 1,
                deleted: false,
                sync_state: SyncState::Synced,
                ..prev.clone()
            },
        };
        self.upsert_item(&item)?;
        tracing::debug!(item_id = %item.id, version = item.version, "text put");
        Ok(outcome(old, item))
    }

    /// 存入或覆盖一个文件：`tmp_path` 为已完整下载/选中的临时文件；
    /// 内部做净化 → 保留名/风险扩展名拦截 → 复制 `.part` → 哈希校验 →
    /// 原子替换 → 旧版转唯一 `.bak` → version+1。
    ///
    /// `expected_sha256` 为 hex（来自 net 层 FileHeader 或调用方自算），
    /// 与 `.part` 实际哈希不符时报 [`StoreError::HashMismatch`] 且不产生正式文件。
    // 接口签名与计划 Task 3 锁定；expected_sha256 为哈希校验（EX-07）必需，无法再精简
    #[allow(clippy::too_many_arguments)]
    pub fn put_file_from_tmp(
        &self,
        owner_id: &str,
        role: Role,
        audience: Audience,
        raw_name: &str,
        tmp_path: &Path,
        expected_sha256: &str,
        now: i64,
    ) -> Result<PutOutcome> {
        let owner = safe_component(owner_id)?;
        // 净化统一拒绝：路径穿越 / 尾随点空格 / NTFS 保留设备名（PR #28）
        // / 非法字符，均以 StoreError::Name(NameReject::*) 上抛
        let name = sanitize_file_name(raw_name)?;
        if classroom_protocol::is_risky_ext(&name) {
            return Err(StoreError::RiskyExt(name));
        }
        // E10（P0，PRD 5.3/9.4/EX-04）：大小上限检查放在 fs::copy **之前**——
        // O(1) metadata 判断，不读内容、不产生 .part，超限立即失败
        let tmp_size = std::fs::metadata(tmp_path)?.len();
        if tmp_size > MAX_FILE_SIZE {
            return Err(StoreError::FileTooLarge(tmp_size));
        }
        let old = self.find_scope(owner, &audience, &name)?;

        let owner_dir = self.repo().join(owner);
        std::fs::create_dir_all(&owner_dir)?;
        let final_path = owner_dir.join(&name);
        let bak_path = owner_dir.join(format!("{name}.bak"));
        let part_path = owner_dir.join(format!("{name}.part"));

        // 复制到仓库内 .part（同卷保证 rename 原子），校验实际落盘字节
        std::fs::copy(tmp_path, &part_path)?;
        let actual = sha256_file(&part_path)?;
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            std::fs::remove_file(&part_path)?;
            return Err(StoreError::HashMismatch {
                expected: expected_sha256.to_string(),
                actual,
            });
        }
        if final_path.exists() {
            std::fs::rename(&final_path, &bak_path)?;
        }
        std::fs::rename(&part_path, &final_path)?;

        let size = std::fs::metadata(&final_path)?.len();
        let item = match &old {
            None => ShareItem {
                id: Uuid::new_v4().to_string(),
                kind: ItemKind::File,
                owner_id: owner.to_string(),
                owner_role: role,
                audience,
                title: name.clone(),
                size,
                sha256: Some(actual),
                rel_path: Some(self.rel_of(&final_path)),
                created_at: now,
                updated_at: now,
                version: 1,
                deleted: false,
                sync_state: SyncState::Synced,
            },
            Some(prev) => ShareItem {
                title: name.clone(),
                size,
                sha256: Some(actual),
                rel_path: Some(self.rel_of(&final_path)),
                updated_at: now,
                version: prev.version + 1,
                deleted: false,
                sync_state: SyncState::Synced,
                ..prev.clone()
            },
        };
        self.upsert_item(&item)?;
        tracing::debug!(item_id = %item.id, version = item.version, "file put");
        Ok(outcome(old, item))
    }

    /// 按条件列出条目。
    pub fn list(&self, f: ListFilter) -> Result<Vec<ShareItem>> {
        let mut sql = format!("SELECT {ITEM_COLS} FROM share_items WHERE 1 = 1");
        let mut args: Vec<String> = Vec::new();
        if !f.include_deleted {
            sql.push_str(" AND deleted = 0");
        }
        if let Some(owner) = &f.owner {
            sql.push_str(" AND owner_id = ?");
            args.push(owner.clone());
        }
        if let Some(audience) = &f.audience {
            sql.push_str(" AND audience = ?");
            args.push(audience_name(audience).to_string());
        }
        if let Some(kind) = &f.kind {
            sql.push_str(" AND kind = ?");
            args.push(kind_name(kind).to_string());
        }
        if let Some(keyword) = &f.keyword {
            sql.push_str(" AND title LIKE ? ESCAPE '\\'");
            args.push(format!("%{}%", escape_like(keyword)));
        }
        sql.push_str(" ORDER BY updated_at DESC, id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(args), row_to_item)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 按 id 查询。
    pub fn get(&self, id: &str) -> Result<Option<ShareItem>> {
        let sql = format!("SELECT {ITEM_COLS} FROM share_items WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        match stmt.query_row(params![id], row_to_item) {
            Ok(item) => Ok(Some(item)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 条目文件实体在仓库内的绝对路径（含防御性穿越检查）。
    pub fn file_body(&self, item: &ShareItem) -> Result<PathBuf> {
        let rel = item
            .rel_path
            .as_deref()
            .ok_or_else(|| StoreError::NotFound(item.id.clone()))?;
        // 防御：rel 是相对 repo 的正斜杠路径，但拒绝任何可能逃逸仓库的成分
        // （含绝对路径前缀；注意不能检查拼接后的完整路径——它天然自带 C: 盘符前缀）
        if Path::new(rel).components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(StoreError::NotFound(item.id.clone()));
        }
        Ok(self.repo().join(rel))
    }

    /// 软删除：写墓碑（deleted=1、DeleteDeferred），不动文件实体。
    pub fn soft_delete(&self, id: &str, now: i64) -> Result<ShareItem> {
        let n = self.conn.execute(
            "UPDATE share_items SET deleted = 1, sync_state = 'DeleteDeferred', updated_at = ?1 \
             WHERE id = ?2 AND deleted = 0",
            params![now, id],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(id.to_string()));
        }
        self.get(id)?
            .ok_or_else(|| StoreError::NotFound(id.to_string()))
    }

    /// 彻底删除条目（PRD 7.4「补删完成」终态）：清实体（若存在）→ 删索引行。
    ///
    /// 幂等：条目不存在时也返回 Ok（重复补删不报错）。事件去重表
    /// （`event_log`）**不清理**——正是它保证重复 delete 事件被按 event_id
    /// 去重（PRD 3.6）。
    pub fn hard_delete(&self, id: &str) -> Result<()> {
        if let Some(item) = self.get(id)? {
            if let Ok(body) = self.file_body(&item) {
                if body.exists() {
                    let _ = std::fs::remove_file(&body);
                }
            }
        }
        self.conn
            .execute("DELETE FROM share_items WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 清理过期墓碑（PRD 7.4 墓碑保留策略）：`deleted=1` 且
    /// `updated_at < older_than` 的条目 → 清实体 + 删索引行，返回清理条数。
    ///
    /// 语义：墓碑在保留期内随 manifest 下发，离线端上线即可补删；超过
    /// 保留期视为「端长期离线、放弃补删」（验收口径见 issue #17 / PR）。
    /// 调用方（host）用「现在 − 保留期」作为 `older_than`。
    pub fn purge_tombstones(&self, older_than: i64) -> Result<usize> {
        let ids: Vec<String> = {
            let sql = "SELECT id FROM share_items WHERE deleted = 1 AND updated_at < ?1";
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map(params![older_than], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut n = 0;
        for id in ids {
            self.hard_delete(&id)?;
            n += 1;
        }
        if n > 0 {
            tracing::info!(count = n, older_than, "过期墓碑已清理");
        }
        Ok(n)
    }

    /// 记录已见事件（event_id 去重表，PRD 3.6）。
    pub fn record_event(&self, ev: &Event) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO event_log (event_id, ts) VALUES (?1, ?2)",
            params![ev.event_id, ev.ts],
        )?;
        Ok(())
    }

    /// 是否已见过该事件。
    pub fn seen_event(&self, event_id: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM event_log WHERE event_id = ?1",
            params![event_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// 应用远端事件：先按 event_id 去重，再按 version 做幂等合并，
    /// 落盘索引并返回合并决策（文件实体由 net 层先下载完再调本函数）。
    pub fn apply_remote(&self, ev: &Event) -> Result<MergeDecision> {
        if self.seen_event(&ev.event_id)? {
            return Ok(MergeDecision::IgnoreDuplicate);
        }
        let old = self.find_scope(&ev.item.owner_id, &ev.item.audience, &ev.item.title)?;
        let decision = merge(old.as_ref(), ev);
        if let MergeDecision::Overwrite(new_item) = &decision {
            // 同作用域旧行（不同 id，如本地自建）让位：列表仅保留一条（PRD 5.7 验收）
            if let Some(prev) = &old {
                if prev.id != new_item.id {
                    self.conn
                        .execute("DELETE FROM share_items WHERE id = ?1", params![prev.id])?;
                }
            }
            self.upsert_item(new_item)?;
        }
        self.record_event(ev)?;
        tracing::debug!(event_id = %ev.event_id, "remote event applied");
        Ok(decision)
    }
}

fn outcome(old: Option<ShareItem>, latest: ShareItem) -> PutOutcome {
    match old {
        Some(prev) => PutOutcome::Overwritten {
            old_version: prev.version,
            latest,
        },
        None => PutOutcome::Created(latest),
    }
}

/// LIKE 通配符转义（`%`、`_`、`\`）。
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
