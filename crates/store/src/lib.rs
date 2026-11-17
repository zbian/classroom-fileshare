//! 课堂共享存储层：SQLite 索引 + 文件仓库。
//!
//! 覆盖语义（PRD 5.7 / 7.3）：作用域键 `owner|audience|名称` 命中时执行
//! `.part → SHA-256 → 原子替换 → 旧版转唯一 .bak → version+1` 的幂等 upsert；
//! 文件名经 [`classroom_protocol::sanitize_file_name`] 净化（剥路径成分，
//! 含绝对路径与 `..` 前缀——净化后只剩文件名、写入 owner 子目录，不可能逃逸），
//! 另加 Windows 保留设备名与风险扩展名拦截（PRD 9.4 / 5.3）。

pub mod files;
pub mod retry;

pub use retry::{RetryEntry, RetryOutcome, RETRY_MAX_ATTEMPTS};

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
    /// 重命名目标名称在同作用域内已被其他条目占用（M2 #19 / PRD 5.4）。
    #[error("名称已被占用: {0}")]
    NameTaken(String),
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
    /// 目标被占用：条目已置 `RetryLocked` 并入退避重试队列（M2 #103，EX-06）。
    ///
    /// 注意：内容此刻**尚未安装**（目标文件旧版仍在），因此该条目**不得**
    /// 广播给其他端；成功重试后由 host 广播。`backed_up=false` 表示旧版
    /// `.bak` 备份也失败（PRD 5.7：不回滚新版本，仅告警）。
    RetryQueued {
        /// 条目（`sync_state = RetryLocked`，`retry_count = 0`）。
        item: ShareItem,
        /// 旧版本备份是否成功。
        backed_up: bool,
    },
}

/// 存储层告警（M2 #103，PRD 5.7）：由调用方 drain 后转 `SvcNotify::Warning`。
#[derive(Debug, Clone, PartialEq)]
pub enum StoreWarning {
    /// 备份（`.bak`）写入失败——新版本已成功覆盖，不回滚（PRD 5.7）。
    BackupFailed {
        /// 条目 id。
        item_id: String,
        /// 标题/文件名。
        title: String,
        /// 细节。
        detail: String,
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
        // M2 #103：DB 不存 retry_count（重试队列在 JSON 持久化层）；
        // store 返回条目时由队列状态覆盖（见 `Store::with_retry_state`）
        retry_count: 0,
    })
}

/// 重命名正文文件（含同名 `.bak`）到新路径；旧文件不存在时视为无需移动
/// （容忍实体已缺失——墓碑/异常场景不阻塞索引更新）。
fn rename_body_pair(old_file: &Path, new_file: &Path) -> Result<()> {
    if old_file == new_file || !old_file.exists() {
        return Ok(());
    }
    std::fs::rename(old_file, new_file)?;
    let mut old_bak = old_file.as_os_str().to_os_string();
    old_bak.push(".bak");
    let old_bak = PathBuf::from(old_bak);
    if old_bak.exists() {
        let mut new_bak = new_file.as_os_str().to_os_string();
        new_bak.push(".bak");
        let _ = std::fs::rename(&old_bak, PathBuf::from(new_bak));
    }
    Ok(())
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
    /// 占用退避重试队列（M2 #103）：内存态 + `data_root/retry_queue.json`
    /// 持久化（原子写）；`open` 时载入（损坏 → 空队列 + warn）。
    retry_queue: std::sync::Mutex<Vec<RetryEntry>>,
    /// 待上报的存储告警（备份失败等，PRD 5.7）；调用方 `take_warnings` 取走。
    warnings: std::sync::Mutex<Vec<StoreWarning>>,
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
            retry_queue: std::sync::Mutex::new(retry::load_queue(root)),
            warnings: std::sync::Mutex::new(Vec::new()),
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
    /// 按作用域键查询条目（M2 #121：学员端发送前判断同名提交，复用 id/version
    /// 以保证本地与教师端一致）。
    pub fn find_by_scope(
        &self,
        owner_id: &str,
        audience: Audience,
        title: &str,
    ) -> Result<Option<ShareItem>> {
        let owner = safe_component(owner_id)?;
        self.find_scope(owner, &audience, title)
    }

    /// 按**外部给定的 id/version** 落库内容（M2 #121，PRD 6.x「我共享的」）。
    ///
    /// 学员本地提交（agent）与教师端学员回传入库（host #94）共用本方法，
    /// 保证**两端 id/version 一致**（本地「我共享的」与教师端看到的同一条目）：
    /// - 同名（同 `owner_id + audience + title`）已有旧行且 id 不同 →
    ///   清掉旧行与旧实体，保证单条（同名覆盖语义）
    /// - 内容按 `kind` 写入（Text：`<owner>/texts/t<hash>.txt`；File：
    ///   `<owner>/<name>`），原子替换 + 旧版 `.bak` 恰 1 份（PRD 7.3）
    /// - `version` / `sync_state` 由调用方给定（学员端 `PendingSync` → FileAck
    ///   后 `Synced`；host 端定位 `Synced`）
    ///
    /// 安全：`owner_id` 经 `safe_component`、标题经 `sanitize_file_name`、
    /// 文件另拒风险扩展名（PRD 9.4 / 5.3）。
    #[allow(clippy::too_many_arguments)]
    pub fn put_entry_with_id(
        &self,
        id: &str,
        owner_id: &str,
        role: Role,
        audience: Audience,
        title: &str,
        kind: ItemKind,
        content: &[u8],
        size: u64,
        sha256: Option<&str>,
        version: u64,
        sync_state: SyncState,
        now: i64,
    ) -> Result<ShareItem> {
        let owner = safe_component(owner_id)?;
        let name = sanitize_file_name(title)?;
        if kind == ItemKind::File && classroom_protocol::is_risky_ext(&name) {
            return Err(StoreError::RiskyExt(name));
        }
        // 同名旧行 → **沿用旧 id**（条目身份稳定、单条；调用方 id 仅新建时
        // 采用），version 兜底取 `max(调用方给定, 旧 version + 1)`——客户端
        // （模拟端/旧端）未递增版本或传入新 id 时，服务端仍保证「同一条目、
        // version+1」（PRD 7.3）。旧行由后续 upsert（同 id）直接替换，无需删除
        // （删除会连带清掉实体，导致覆盖前无法生成 `.bak`）。
        let (created_at, version, id) = match self.find_scope(owner, &audience, &name)? {
            Some(prev) => (
                prev.created_at,
                version.max(prev.version + 1),
                prev.id.clone(),
            ),
            None => (now, version, id.to_string()),
        };
        // 内容写入（原子替换 + 旧版 .bak 恰 1 份）
        let owner_dir = self.repo().join(owner);
        let final_path = match kind {
            ItemKind::Text => {
                let dir = owner_dir.join("texts");
                std::fs::create_dir_all(&dir)?;
                let h = scope_hash(owner, &audience, &name);
                dir.join(format!("t{h}.txt"))
            }
            ItemKind::File => {
                std::fs::create_dir_all(&owner_dir)?;
                owner_dir.join(&name)
            }
        };
        let mut bak_os = final_path.as_os_str().to_os_string();
        bak_os.push(".bak");
        let bak_path = PathBuf::from(bak_os);
        let mut part_os = final_path.as_os_str().to_os_string();
        part_os.push(".part");
        let write_path = PathBuf::from(part_os);
        std::fs::write(&write_path, content)?;
        if final_path.exists() {
            let _ = std::fs::remove_file(&bak_path);
            std::fs::rename(&final_path, &bak_path)?;
        }
        std::fs::rename(&write_path, &final_path)?;
        let item = ShareItem {
            id: id.to_string(),
            kind,
            owner_id: owner.to_string(),
            owner_role: role,
            audience,
            title: name,
            size,
            sha256: sha256.map(|s| s.to_string()),
            rel_path: Some(self.rel_of(&final_path)),
            created_at,
            updated_at: now,
            version,
            deleted: false,
            sync_state,
            retry_count: 0,
        };
        self.upsert_item(&item)?;
        tracing::debug!(
            item_id = %item.id,
            version = item.version,
            kind = ?item.kind,
            "entry put (with id)"
        );
        Ok(item)
    }

    /// 设置条目同步状态（M2 #117：接收端占用重试 `RetryLocked` / 完成
    /// `Synced` / 耗尽 `Failed`）。
    pub fn set_sync_state(&self, id: &str, state: SyncState) -> Result<Option<ShareItem>> {
        let n = self.conn.execute(
            "UPDATE share_items SET sync_state = ?1 WHERE id = ?2",
            params![sync_state_name(&state), id],
        )?;
        if n == 0 {
            return Ok(None);
        }
        self.get(id)
    }

    /// 标记条目为 `Synced`（M2 #121：学员本地提交收到 host `FileAck{ok}` 后）。
    pub fn mark_synced(&self, id: &str) -> Result<Option<ShareItem>> {
        let n = self.conn.execute(
            "UPDATE share_items SET sync_state = 'Synced' WHERE id = ?1",
            params![id],
        )?;
        if n == 0 {
            return Ok(None);
        }
        self.get(id)
    }

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
                retry_count: 0,
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
                retry_count: 0,
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
        // M2 #103（PRD 5.7）：备份失败**不回滚**已成功的新版本，仅记告警；
        // 目标仍被占用 → 内容持久到重试目录 + 条目 RetryLocked 入队。
        let mut backed_up = true;
        if final_path.exists() {
            if let Err(e) = std::fs::rename(&final_path, &bak_path) {
                // 备份失败：继续覆盖（新版本落盘），旧版无 .bak
                backed_up = false;
                tracing::warn!(title = %name, error = %e, "备份 .bak 失败（不回滚新版本，记告警）");
            }
        }
        match std::fs::rename(&part_path, &final_path) {
            Ok(()) => {}
            Err(e) if retry::is_target_busy(&e) => {
                // 目标被占用：入退避重试队列（内容持久化到 data_root/retry/）
                let item = self.enqueue_retry(
                    owner,
                    role,
                    audience,
                    &name,
                    &part_path,
                    &actual,
                    old.as_ref(),
                    now,
                    backed_up,
                )?;
                return Ok(PutOutcome::RetryQueued { item, backed_up });
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part_path);
                if !backed_up {
                    // 备份已失败且安装也失败：旧版已改名为 .bak（存在），回滚
                    if bak_path.exists() && !final_path.exists() {
                        let _ = std::fs::rename(&bak_path, &final_path);
                    }
                }
                return Err(e.into());
            }
        }
        if !backed_up {
            // 安装成功但旧版备份失败：记告警（PRD 5.7）
            let id_hint = old
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| "pending".to_string());
            self.push_warning(StoreWarning::BackupFailed {
                item_id: id_hint,
                title: name.clone(),
                detail: "旧版本 .bak 备份失败（新版本已成功落盘，未回滚）".to_string(),
            });
        }

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
                retry_count: 0,
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
                retry_count: 0,
                ..prev.clone()
            },
        };
        self.upsert_item(&item)?;
        tracing::debug!(item_id = %item.id, version = item.version, "file put");
        Ok(outcome(old, item))
    }

    // ---------- M2 #103：占用退避重试队列（PRD 5.7 / 7.3 / EX-06） ----------

    /// 占用失败入队：内容持久到 `data_root/retry/<id>.part`（不放 %TEMP%，
    /// 重启后仍可重试），条目置 `RetryLocked`，队列原子落盘。
    #[allow(clippy::too_many_arguments)]
    fn enqueue_retry(
        &self,
        owner: &str,
        role: Role,
        audience: Audience,
        name: &str,
        part_path: &Path,
        sha256: &str,
        old: Option<&ShareItem>,
        now: i64,
        backed_up: bool,
    ) -> Result<ShareItem> {
        let (id, version, created_at) = match old {
            None => (Uuid::new_v4().to_string(), 1, now),
            Some(prev) => (prev.id.clone(), prev.version + 1, prev.created_at),
        };
        // 内容持久化（跨卷 copy 安全；成功/耗尽后清理）
        let dir = retry::retry_dir(&self.root);
        std::fs::create_dir_all(&dir)?;
        let dest = dir.join(format!("{id}.part"));
        std::fs::copy(part_path, &dest)?;
        let _ = std::fs::remove_file(part_path);
        let final_path = self.repo().join(owner).join(name);
        let item = ShareItem {
            id: id.clone(),
            kind: ItemKind::File,
            owner_id: owner.to_string(),
            owner_role: role,
            audience,
            title: name.to_string(),
            size: std::fs::metadata(&dest)?.len(),
            sha256: Some(sha256.to_string()),
            rel_path: Some(self.rel_of(&final_path)),
            created_at,
            updated_at: now,
            version,
            deleted: false,
            sync_state: SyncState::RetryLocked,
            retry_count: 0,
        };
        self.upsert_item(&item)?;
        {
            let mut queue = self.retry_queue.lock().unwrap();
            // 同一条目不重复入队（重复 put / 并发兜底）
            if !queue.iter().any(|e| e.item_id == item.id) {
                queue.push(RetryEntry {
                    item_id: item.id.clone(),
                    rel_part: format!("retry/{id}.part"),
                    attempts: 0,
                    next_at: now, // 立即可试；退避由调用方（host 调度器）计算
                    file_name: name.to_string(),
                    owner_id: owner.to_string(),
                    sha256: sha256.to_string(),
                    created_at: now,
                });
            }
            if let Err(e) = retry::save_queue(&self.root, &queue) {
                tracing::warn!(error = %e, "重试队列落盘失败（内存态继续）");
            }
        }
        if !backed_up {
            self.push_warning(StoreWarning::BackupFailed {
                item_id: item.id.clone(),
                title: name.to_string(),
                detail: "旧版本备份失败（目标被占用）".to_string(),
            });
        }
        tracing::info!(item_id = %item.id, "目标被占用：条目进入退避重试队列");
        Ok(item)
    }

    /// 队列快照（观测：重试次数 / 下次重试时间）。
    pub fn retry_snapshot(&self) -> Vec<RetryEntry> {
        self.retry_queue.lock().unwrap().clone()
    }

    /// 到期待重试项（`next_at <= now`）。
    pub fn retry_due(&self, now: i64) -> Vec<RetryEntry> {
        self.retry_queue
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.next_at <= now)
            .cloned()
            .collect()
    }

    /// 执行一次重试（占用 → 退避 +1；成功 → Synced；耗尽 → Failed）。
    ///
    /// `initial_backoff_secs` 由 host 配置（默认 1s，测试可注入更短值）。
    pub fn retry_attempt(
        &self,
        item_id: &str,
        now: i64,
        initial_backoff_secs: i64,
    ) -> Result<RetryOutcome> {
        let entry = self
            .retry_queue
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.item_id == item_id)
            .cloned();
        let Some(entry) = entry else {
            return Ok(RetryOutcome::Aborted {
                item_id: item_id.to_string(),
                reason: "队列项不存在".to_string(),
            });
        };
        // 内容校验（重启后内容仍在且未被篡改）
        let part = self.root.join(&entry.rel_part);
        let content_ok = part.exists()
            && sha256_file(&part)
                .map(|h| h.eq_ignore_ascii_case(&entry.sha256))
                .unwrap_or(false);
        if !content_ok {
            self.retry_remove(item_id)?;
            self.mark_item_state(item_id, SyncState::Failed)?;
            return Ok(RetryOutcome::Aborted {
                item_id: item_id.to_string(),
                reason: "重试内容缺失或哈希不符".to_string(),
            });
        }
        let owner_dir = self.repo().join(&entry.owner_id);
        std::fs::create_dir_all(&owner_dir)?;
        let final_path = owner_dir.join(&entry.file_name);
        let bak_path = owner_dir.join(format!("{}.bak", entry.file_name));
        let mut backed_up = true;
        if final_path.exists() {
            if let Err(e) = std::fs::rename(&final_path, &bak_path) {
                backed_up = false;
                tracing::warn!(title = %entry.file_name, error = %e, "重试时备份失败（不回滚）");
            }
        }
        match std::fs::rename(&part, &final_path) {
            Ok(()) => {
                self.retry_remove(item_id)?;
                self.mark_item_state(item_id, SyncState::Synced)?;
                if !backed_up {
                    self.push_warning(StoreWarning::BackupFailed {
                        item_id: item_id.to_string(),
                        title: entry.file_name.clone(),
                        detail: "重试成功后旧版本备份失败".to_string(),
                    });
                }
                tracing::info!(item_id = %item_id, "占用重试成功");
                Ok(RetryOutcome::Succeeded {
                    item_id: item_id.to_string(),
                })
            }
            Err(e) if retry::is_target_busy(&e) => {
                let attempts = entry.attempts + 1;
                if attempts >= retry::RETRY_MAX_ATTEMPTS {
                    self.retry_remove(item_id)?;
                    self.mark_item_state(item_id, SyncState::Failed)?;
                    tracing::warn!(item_id = %item_id, attempts, "占用重试耗尽，条目转 Failed");
                    Ok(RetryOutcome::Exhausted {
                        item_id: item_id.to_string(),
                    })
                } else {
                    let next_at = now + retry::backoff_secs(attempts, initial_backoff_secs);
                    self.retry_update_attempt(item_id, attempts, next_at)?;
                    Ok(RetryOutcome::StillBusy {
                        item_id: item_id.to_string(),
                        attempts,
                        next_at,
                    })
                }
            }
            Err(e) => {
                self.retry_remove(item_id)?;
                self.mark_item_state(item_id, SyncState::Failed)?;
                Ok(RetryOutcome::Aborted {
                    item_id: item_id.to_string(),
                    reason: e.to_string(),
                })
            }
        }
    }

    /// 取走待上报的存储告警（备份失败等）——调用方转 `SvcNotify::Warning`。
    pub fn take_warnings(&self) -> Vec<StoreWarning> {
        std::mem::take(&mut *self.warnings.lock().unwrap())
    }

    fn push_warning(&self, w: StoreWarning) {
        self.warnings.lock().unwrap().push(w);
    }

    /// 队列移除 + 清理持久内容 + 落盘。
    fn retry_remove(&self, item_id: &str) -> Result<()> {
        let mut queue = self.retry_queue.lock().unwrap();
        if let Some(pos) = queue.iter().position(|e| e.item_id == item_id) {
            let entry = queue.remove(pos);
            let part = self.root.join(&entry.rel_part);
            let _ = std::fs::remove_file(part);
            if let Err(e) = retry::save_queue(&self.root, &queue) {
                tracing::warn!(error = %e, "重试队列落盘失败");
            }
        }
        Ok(())
    }

    fn retry_update_attempt(&self, item_id: &str, attempts: u32, next_at: i64) -> Result<()> {
        let mut queue = self.retry_queue.lock().unwrap();
        if let Some(e) = queue.iter_mut().find(|e| e.item_id == item_id) {
            e.attempts = attempts;
            e.next_at = next_at;
        }
        if let Err(err) = retry::save_queue(&self.root, &queue) {
            tracing::warn!(error = %err, "重试队列落盘失败");
        }
        Ok(())
    }

    fn mark_item_state(&self, item_id: &str, state: SyncState) -> Result<()> {
        self.conn.execute(
            "UPDATE share_items SET sync_state = ?1 WHERE id = ?2",
            params![sync_state_name(&state), item_id],
        )?;
        Ok(())
    }

    /// 返回条目时叠加重试状态（`retry_count` / `sync_state`）。
    ///
    /// DB 不存 retry_count（重试队列在 JSON 层）。队列项存在 →
    /// `RetryLocked` + 当前 attempts；`Failed` 且不在队列 → `retry_count`
    /// 取上限（GUI 显示「已用尽」）。
    fn with_retry_state(&self, mut item: ShareItem) -> ShareItem {
        let queue = self.retry_queue.lock().unwrap();
        if let Some(e) = queue.iter().find(|e| e.item_id == item.id) {
            item.retry_count = e.attempts;
            item.sync_state = SyncState::RetryLocked;
        } else if item.sync_state == SyncState::Failed {
            item.retry_count = retry::RETRY_MAX_ATTEMPTS;
        }
        item
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
        sql.push_str(" ORDER BY updated_at DESC, id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(args), row_to_item)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // M2 #19（PRD 5.4「按名称与内容搜索」）：标题或**文本正文**包含关键字。
        // 正文存放在文件里、SQL 无法直接检索 → 内存过滤（M2 条目量级可接受；
        // M3 若需要可加全文索引列）。
        let rows: Vec<ShareItem> = if let Some(keyword) = &f.keyword {
            let kw_lower = keyword.to_lowercase();
            rows.into_iter()
                .filter(|item| self.item_matches_keyword(item, &kw_lower))
                .collect()
        } else {
            rows
        };
        // M2 #103：叠加重试状态（RetryLocked / retry_count），GUI 据此显示「第 n/5 次」
        Ok(rows
            .into_iter()
            .map(|item| self.with_retry_state(item))
            .collect())
    }

    /// 关键字命中判定（PRD 5.4）：标题包含，或文本条目正文包含（忽略大小写）。
    ///
    /// 文件正文不参与搜索（M2 仅文本；PRD 5.4 的「内容」指文本正文）。
    fn item_matches_keyword(&self, item: &ShareItem, kw_lower: &str) -> bool {
        if item.title.to_lowercase().contains(kw_lower) {
            return true;
        }
        if item.kind != ItemKind::Text {
            return false;
        }
        let Ok(path) = self.file_body(item) else {
            return false;
        };
        match std::fs::read_to_string(path) {
            Ok(body) => body.to_lowercase().contains(kw_lower),
            Err(_) => false,
        }
    }

    /// 按 id 查询。
    pub fn get(&self, id: &str) -> Result<Option<ShareItem>> {
        let sql = format!("SELECT {ITEM_COLS} FROM share_items WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        match stmt.query_row(params![id], row_to_item) {
            Ok(item) => Ok(Some(self.with_retry_state(item))),
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
    /// 重命名条目（M2 #19，PRD 5.4 FR-T-04：文本与文件通用）。
    ///
    /// 语义：
    /// - 新名经 [`sanitize_file_name`] 净化（剥路径成分）；文件条目另拒风险
    ///   扩展名（PRD 5.3）
    /// - 同作用域（owner+audience+新名）已有**其他**未删除条目 →
    ///   [`StoreError::NameTaken`]（M2 拒绝而非静默覆盖：避免误删数据；
    ///   PRD 5.4 的「按 5.7 覆盖」语义留 M3）
    /// - 文本：正文文件按新 scope hash 改名；文件：实体与 `.bak` 一并改名
    /// - `version + 1`、`updated_at = now`、索引同步更新（各端按 version 应用）
    pub fn rename_item(&self, id: &str, new_title: &str, now: i64) -> Result<ShareItem> {
        let Some(item) = self.get(id)? else {
            return Err(StoreError::NotFound(id.to_string()));
        };
        if item.deleted {
            return Err(StoreError::NotFound(id.to_string()));
        }
        let name = sanitize_file_name(new_title)?;
        if item.kind == ItemKind::File && classroom_protocol::is_risky_ext(&name) {
            return Err(StoreError::RiskyExt(name));
        }
        if let Some(prev) = self.find_scope(&item.owner_id, &item.audience, &name)? {
            if prev.id != item.id {
                return Err(StoreError::NameTaken(name));
            }
        }
        let old_rel = item.rel_path.clone().unwrap_or_default();
        let old_file = self.repo().join(&old_rel);
        let owner_dir = self.repo().join(&item.owner_id);
        let new_rel = match item.kind {
            ItemKind::Text => {
                std::fs::create_dir_all(owner_dir.join("texts"))?;
                let h = scope_hash(&item.owner_id, &item.audience, &name);
                let new_file = owner_dir.join("texts").join(format!("t{h}.txt"));
                rename_body_pair(&old_file, &new_file)?;
                self.rel_of(&new_file)
            }
            ItemKind::File => {
                std::fs::create_dir_all(&owner_dir)?;
                let new_file = owner_dir.join(&name);
                rename_body_pair(&old_file, &new_file)?;
                self.rel_of(&new_file)
            }
        };
        let updated = ShareItem {
            title: name,
            rel_path: Some(new_rel),
            updated_at: now,
            version: item.version + 1,
            ..item
        };
        self.upsert_item(&updated)?;
        tracing::info!(
            item_id = %updated.id,
            version = updated.version,
            "rename"
        );
        Ok(updated)
    }

    pub fn soft_delete(&self, id: &str, now: i64) -> Result<ShareItem> {
        // M2 #103：删除条目即取消其占位重试（不再重试已删除内容）
        self.retry_remove(id)?;
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
        // M2 #103：彻底删除同时取消占位重试
        self.retry_remove(id)?;
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

    /// 删除条目的文件实体（final + `.bak`），保留索引行（墓碑）——供离线端
    /// 补删与 manifest 比对（M2 #17 / TC-08 观察项 O-A：PRD 7.4 字面
    /// 「已删除：删除实体、写墓碑、广播 delete」，此前实体滞留在 TTL purge）。
    ///
    /// 容忍实体已不存在（幂等；`hard_delete`/重复调用安全）。
    pub fn remove_item_bodies(&self, item: &ShareItem) -> Result<()> {
        let Ok(final_path) = self.file_body(item) else {
            return Ok(());
        };
        if final_path.exists() {
            if let Err(e) = std::fs::remove_file(&final_path) {
                tracing::warn!(path = %final_path.display(), error = %e, "删除条目实体失败");
            }
        }
        let mut bak = final_path.as_os_str().to_os_string();
        bak.push(".bak");
        let bak_path = PathBuf::from(bak);
        if bak_path.exists() {
            if let Err(e) = std::fs::remove_file(&bak_path) {
                tracing::warn!(path = %bak_path.display(), error = %e, "删除条目备份失败");
            }
        }
        Ok(())
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
