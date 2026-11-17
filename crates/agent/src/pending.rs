//! 离线待发队列（M2 #18，PRD 6.8 / 7.5 / 9.2）。
//!
//! 与 #103 的**占用重试队列**语义不同，勿混：
//! - #103（host 侧）：写目标被其他程序占用 → 退避重试同一次写入；
//! - 本模块（agent 侧）：**教师机不可达**（未连接/断线）时把学员提交操作
//!   暂存，恢复连接后按操作时间顺序补发。
//!
//! 持久化：
//! - 队列：`data_root/pending/queue.json`（**原子写**：`.tmp` → rename）
//! - 待发内容：`data_root/pending/<op_id>.bin`（文本正文或文件字节；
//!   `%TEMP%` 会被系统清理，**不放那里**）
//! - 启动读失败/损坏 → 空队列 + warn，不 panic（断电不丢：正常关闭/断电后
//!   已 fsync 的内容与队列可恢复）
//!
//! 补发规则（PRD 6.8）：按 `created_at` 顺序；失败项按退避重试并记次数；
//! **单条失败不阻塞整条队列**；成功后删除该条与内容。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 待发内容类型。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingKind {
    /// 文本（正文在内容文件里，标题在 `title`）。
    Text,
    /// 文件（文件名字节在内容文件里，`title` 为文件名）。
    File,
}

/// 一条待发操作（持久化结构）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PendingOp {
    /// 操作 id（同时是内容文件名）。
    pub id: String,
    /// 类型。
    pub kind: PendingKind,
    /// 文本标题 / 文件名。
    pub title: String,
    /// 内容文件相对路径（`pending/<id>.bin`，相对 data_root）。
    pub content_rel: String,
    /// 内容字节数。
    pub size: u64,
    /// 内容 SHA-256（hex）。
    pub sha256: String,
    /// 操作时间（Unix 秒）——补发按此升序。
    pub created_at: i64,
    /// 已完成补发尝试次数。
    pub attempts: u32,
    /// 下次允许补发时间（退避）。
    pub next_at: i64,
}

/// 待发内容目录（`data_root/pending/`）。
pub fn pending_dir(root: &Path) -> PathBuf {
    root.join("pending")
}

fn queue_path(root: &Path) -> PathBuf {
    pending_dir(root).join("queue.json")
}

/// 原子写队列（`.tmp` → rename）。
pub fn save(root: &Path, ops: &[PendingOp]) -> std::io::Result<()> {
    let dir = pending_dir(root);
    std::fs::create_dir_all(&dir)?;
    let path = queue_path(root);
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(ops)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// 读队列（不存在/损坏 → 空 + warn）。
pub fn load(root: &Path) -> Vec<PendingOp> {
    let path = queue_path(root);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Vec<PendingOp>>(&bytes) {
            Ok(ops) => ops,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "待发队列损坏，按空队列继续");
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "待发队列读取失败，按空队列继续");
            Vec::new()
        }
    }
}

/// 待发队列（内存态 + 持久化）。
///
/// 所有变更方法都先改内存再原子落盘；落盘失败仅 `warn`（内存态继续，
/// 保可用性优先——与 PRD 9.2「不自动删除未同步内容」一致）。
pub struct PendingQueue {
    root: PathBuf,
    ops: Vec<PendingOp>,
}

impl PendingQueue {
    /// 打开（载入持久化队列）。
    pub fn open(root: &Path) -> Self {
        let mut ops = load(root);
        // 按操作时间排序（补发顺序稳定；防手工编辑损坏顺序）
        ops.sort_by_key(|o| (o.created_at, o.id.clone()));
        // 内容缺失的条目丢弃（文件被外部清理——不阻塞队列）
        let before = ops.len();
        ops.retain(|o| root.join(&o.content_rel).exists());
        if ops.len() != before {
            tracing::warn!(
                dropped = before - ops.len(),
                "待发队列中部分内容缺失，已丢弃（不自动删除其他未同步内容）"
            );
        }
        let q = Self {
            root: root.to_path_buf(),
            ops,
        };
        if before != q.ops.len() {
            let _ = save(root, &q.ops);
        }
        q
    }

    /// 队列长度。
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// 待发内容总字节数（缓存阈值告警用）。
    pub fn total_bytes(&self) -> u64 {
        self.ops.iter().map(|o| o.size).sum()
    }

    /// 只读快照（按操作时间升序）。
    pub fn snapshot(&self) -> Vec<PendingOp> {
        self.ops.clone()
    }

    /// 入队（内容文件由调用方先写入 `pending/<id>.bin`）。
    pub fn push(&mut self, op: PendingOp) {
        // 同 id 不重复入队（幂等兜底）
        if !self.ops.iter().any(|o| o.id == op.id) {
            self.ops.push(op);
            self.ops.sort_by_key(|o| (o.created_at, o.id.clone()));
            self.persist();
        }
    }

    /// 到期待补发项（按操作时间升序，`next_at <= now`）。
    pub fn due(&self, now: i64) -> Vec<PendingOp> {
        let mut due: Vec<PendingOp> = self
            .ops
            .iter()
            .filter(|o| o.next_at <= now)
            .cloned()
            .collect();
        due.sort_by_key(|o| (o.created_at, o.id.clone()));
        due
    }

    /// 补发成功：移出队列并删除内容文件。
    pub fn complete(&mut self, id: &str) {
        if let Some(pos) = self.ops.iter().position(|o| o.id == id) {
            let op = self.ops.remove(pos);
            let _ = std::fs::remove_file(self.root.join(&op.content_rel));
            self.persist();
        }
    }

    /// 补发失败：attempts+1，`next_at = now + backoff`（指数退避 1s 起，封顶 60s）。
    pub fn retry_later(&mut self, id: &str, now: i64) -> u32 {
        let mut attempts = 0;
        if let Some(op) = self.ops.iter_mut().find(|o| o.id == id) {
            op.attempts = op.attempts.saturating_add(1);
            attempts = op.attempts;
            let delay = (1i64 << attempts.saturating_sub(1).min(6)).min(60);
            op.next_at = now + delay;
        }
        self.persist();
        attempts
    }

    /// 丢弃（内容损坏等不可重试场景）。
    pub fn drop_op(&mut self, id: &str) {
        self.complete(id);
    }

    fn persist(&self) {
        if let Err(e) = save(&self.root, &self.ops) {
            tracing::warn!(error = %e, "待发队列落盘失败（内存态继续）");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: &str, created_at: i64) -> PendingOp {
        PendingOp {
            id: id.into(),
            kind: PendingKind::Text,
            title: "作业".into(),
            content_rel: format!("pending/{id}.bin"),
            size: 3,
            sha256: "00".repeat(32),
            created_at,
            attempts: 0,
            next_at: 0,
        }
    }

    fn write_content(root: &Path, id: &str) {
        std::fs::create_dir_all(pending_dir(root)).unwrap();
        std::fs::write(pending_dir(root).join(format!("{id}.bin")), b"abc").unwrap();
    }

    #[test]
    fn queue_roundtrip_and_order_by_created_at() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_content(root, "b");
        write_content(root, "a");
        let mut q = PendingQueue::open(root);
        q.push(op("b", 200));
        q.push(op("a", 100));
        assert_eq!(q.len(), 2);
        // 按操作时间升序补发
        assert_eq!(q.due(1000)[0].id, "a");
        // 重开持久化
        let q2 = PendingQueue::open(root);
        assert_eq!(q2.len(), 2);
        assert_eq!(q2.snapshot()[0].id, "a");
    }

    #[test]
    fn complete_removes_entry_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_content(root, "x");
        let mut q = PendingQueue::open(root);
        q.push(op("x", 10));
        q.complete("x");
        assert!(q.is_empty());
        assert!(!pending_dir(root).join("x.bin").exists(), "内容已清理");
        assert!(PendingQueue::open(root).is_empty(), "持久化同步");
    }

    #[test]
    fn retry_backs_off_exponentially_and_capped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_content(root, "r");
        let mut q = PendingQueue::open(root);
        q.push(op("r", 10));
        assert_eq!(q.retry_later("r", 1000), 1);
        assert_eq!(q.snapshot()[0].next_at, 1001);
        assert_eq!(q.retry_later("r", 2000), 2);
        assert_eq!(q.snapshot()[0].next_at, 2002);
        // 退避封顶 60s
        for _ in 0..10 {
            q.retry_later("r", 3000);
        }
        let op = &q.snapshot()[0];
        assert!(op.next_at <= 3000 + 60, "退避封顶 60s: {}", op.next_at);
    }

    #[test]
    fn missing_content_is_dropped_without_blocking_other_ops() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_content(root, "alive");
        // 只有 alive 的内容存在；"gone" 的内容缺失
        let mut q = PendingQueue::open(root);
        q.push(op("alive", 100));
        // 手工塞入一个内容缺失的条目（模拟外部清理）
        let mut ops = load(root);
        ops.push(op("gone", 50));
        save(root, &ops).unwrap();
        let q2 = PendingQueue::open(root);
        assert_eq!(q2.len(), 1, "内容缺失条目被丢弃，其余保留");
        assert_eq!(q2.snapshot()[0].id, "alive");
        assert_eq!(q2.due(1000).len(), 1);
    }
}
