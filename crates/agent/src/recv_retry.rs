//! 学员**接收端**占用重试队列（M2 #117，PRD 5.7 / EX-06 接收侧）。
//!
//! 与教师侧 #103 的写入重试队列同一契约，但作用在 agent 接收落盘：
//! 教师广播同名新版、学员本地旧版正被阅读器/编辑器锁定 → `rename` 失败
//! （Windows `ERROR_SHARING_VIOLATION` / `ERROR_ACCESS_DENIED`）→ **不抛错、
//! 不断连**，条目置 `RetryLocked`、内容持久到 `data_root/retry_recv/`，
//! 退避重试 ≤5 次：成功 → `Synced` + `.bak` 恰 1 份；耗尽 → `Failed` + 告警。
//!
//! 持久化：`data_root/recv_retry.json`（**原子写**，损坏 → 空队列 + warn）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 接收端最大重试次数（与教师侧 #103 一致，PRD EX-06）。
pub const RECV_RETRY_MAX_ATTEMPTS: u32 = 5;

/// 队列文件名（位于 data_root 下）。
const QUEUE_FILE: &str = "recv_retry.json";
/// 待安装内容目录（位于 data_root 下）。
const RETRY_DIR: &str = "retry_recv";

/// 接收端重试项（持久化结构）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RecvRetryEntry {
    /// 条目 id。
    pub item_id: String,
    /// 最终安装路径（repo 相对，来自教师 Event 的 `rel_path`）。
    pub rel_path: String,
    /// 待安装内容相对路径（`retry_recv/<item_id>.part`，相对 data_root）。
    pub part_rel: String,
    /// 内容整体 SHA-256（hex，重试安装前复校验）。
    pub sha256: String,
    /// 已完成重试次数。
    pub attempts: u32,
    /// 下次重试时间（Unix 秒）。
    pub next_at: i64,
    /// 入队时间（Unix 秒）。
    pub created_at: i64,
}

/// 待安装内容路径（`data_root/retry_recv/`）。
pub fn retry_content_dir(root: &Path) -> PathBuf {
    root.join(RETRY_DIR)
}

/// 队列文件路径（`data_root/recv_retry.json`）。
pub fn queue_path(root: &Path) -> PathBuf {
    root.join(QUEUE_FILE)
}

/// 原子写队列（`.tmp` → rename）。
pub fn save(root: &Path, entries: &[RecvRetryEntry]) -> std::io::Result<()> {
    let path = queue_path(root);
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(entries)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// 读队列（不存在/损坏 → 空 + warn，不 panic）。
pub fn load(root: &Path) -> Vec<RecvRetryEntry> {
    let path = queue_path(root);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Vec<RecvRetryEntry>>(&bytes) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "接收端重试队列损坏，按空队列继续");
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "接收端重试队列读取失败，按空队列继续");
            Vec::new()
        }
    }
}

/// 接收端重试队列（内存态 + 持久化）。
pub struct RecvRetryQueue {
    root: PathBuf,
    entries: Vec<RecvRetryEntry>,
}

impl RecvRetryQueue {
    /// 打开（载入持久化队列；内容缺失的条目丢弃，不阻塞其他条目）。
    pub fn open(root: &Path) -> Self {
        let mut entries = load(root);
        let before = entries.len();
        entries.retain(|e| root.join(&e.part_rel).exists());
        if entries.len() != before {
            tracing::warn!(
                dropped = before - entries.len(),
                "接收端重试队列部分内容缺失，已丢弃（不阻塞其他条目）"
            );
        }
        let q = Self {
            root: root.to_path_buf(),
            entries,
        };
        if entries_len_changed(&q.entries, before) {
            let _ = save(root, &q.entries);
        }
        q
    }

    /// 队列长度。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 快照。
    pub fn snapshot(&self) -> Vec<RecvRetryEntry> {
        self.entries.clone()
    }

    /// 入队（同 item 不重复；内容文件由调用方先写入 `retry_recv/<id>.part`）。
    pub fn push(&mut self, entry: RecvRetryEntry) {
        if !self.entries.iter().any(|e| e.item_id == entry.item_id) {
            self.entries.push(entry);
            self.persist();
        }
    }

    /// 到期待重试项（按入队时间升序）。
    pub fn due(&self, now: i64) -> Vec<RecvRetryEntry> {
        let mut due: Vec<RecvRetryEntry> = self
            .entries
            .iter()
            .filter(|e| e.next_at <= now)
            .cloned()
            .collect();
        due.sort_by_key(|e| (e.created_at, e.item_id.clone()));
        due
    }

    /// 仍被占用：attempts+1，`next_at = now + 2^(attempts-1)`（1s 起，封顶 60s）。
    pub fn retry_later(&mut self, item_id: &str, now: i64) -> u32 {
        let mut attempts = 0;
        if let Some(e) = self.entries.iter_mut().find(|e| e.item_id == item_id) {
            e.attempts = e.attempts.saturating_add(1);
            attempts = e.attempts;
            let delay = (1i64 << attempts.saturating_sub(1).min(6)).min(60);
            e.next_at = now + delay;
        }
        self.persist();
        attempts
    }

    /// 完成/放弃：移除条目并删除内容文件。
    pub fn remove(&mut self, item_id: &str) {
        if let Some(pos) = self.entries.iter().position(|e| e.item_id == item_id) {
            let e = self.entries.remove(pos);
            let _ = std::fs::remove_file(self.root.join(&e.part_rel));
            self.persist();
        }
    }

    fn persist(&self) {
        if let Err(e) = save(&self.root, &self.entries) {
            tracing::warn!(error = %e, "接收端重试队列落盘失败（内存态继续）");
        }
    }
}

fn entries_len_changed(entries: &[RecvRetryEntry], before: usize) -> bool {
    entries.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, created_at: i64) -> RecvRetryEntry {
        RecvRetryEntry {
            item_id: id.into(),
            rel_path: format!("teacher/{id}.pdf"),
            part_rel: format!("retry_recv/{id}.part"),
            sha256: "00".repeat(32),
            attempts: 0,
            next_at: 0,
            created_at,
        }
    }

    fn write_content(root: &Path, id: &str) {
        std::fs::create_dir_all(retry_content_dir(root)).unwrap();
        std::fs::write(retry_content_dir(root).join(format!("{id}.part")), b"abc").unwrap();
    }

    #[test]
    fn queue_roundtrip_and_due_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_content(root, "b");
        write_content(root, "a");
        let mut q = RecvRetryQueue::open(root);
        q.push(entry("b", 200));
        q.push(entry("a", 100));
        assert_eq!(q.len(), 2);
        assert_eq!(q.due(1000)[0].item_id, "a", "按入队时间升序重试");
        let q2 = RecvRetryQueue::open(root);
        assert_eq!(q2.len(), 2, "持久化往返");
    }

    #[test]
    fn remove_clears_entry_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_content(root, "x");
        let mut q = RecvRetryQueue::open(root);
        q.push(entry("x", 10));
        q.remove("x");
        assert!(q.is_empty());
        assert!(!retry_content_dir(root).join("x.part").exists());
        assert!(RecvRetryQueue::open(root).is_empty());
    }

    #[test]
    fn retry_backoff_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_content(root, "r");
        let mut q = RecvRetryQueue::open(root);
        q.push(entry("r", 10));
        assert_eq!(q.retry_later("r", 1000), 1);
        assert_eq!(q.snapshot()[0].next_at, 1001);
        assert_eq!(q.retry_later("r", 2000), 2);
        assert_eq!(q.snapshot()[0].next_at, 2002);
        for _ in 0..10 {
            q.retry_later("r", 3000);
        }
        assert!(q.snapshot()[0].next_at <= 3000 + 60, "退避封顶 60s");
    }

    #[test]
    fn corrupt_queue_falls_back_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(queue_path(root), b"{ not valid json").unwrap();
        assert!(
            RecvRetryQueue::open(root).is_empty(),
            "损坏 → 空队列不 panic"
        );
    }
}
