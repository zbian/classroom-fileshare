//! 占用退避重试队列（M2 #103，PRD 5.7 / 7.3 / EX-06）。
//!
//! 背景：Windows 上目标文件被其他程序占用时，原子替换（rename）会失败
//! （`ERROR_SHARING_VIOLATION` / `ERROR_ACCESS_DENIED`）。本模块把这种
//! **可恢复失败**从「直接报错丢弃」升级为：条目置 `RetryLocked` 入队 →
//! 退避重试（≤5 次）→ 成功转 `Synced`（内容最新、`.bak` 恰 1 份）/ 耗尽转
//! `Failed` + 告警。
//!
//! 持久化：`data_root/retry_queue.json`（**原子写**：先写 `.tmp` 再 rename，
//! 避免断电留下半截 JSON）；启动读失败（损坏/不存在）→ 空队列 + warn，
//! 不 panic。
//!
//! 内容持久：待安装内容保存在 `data_root/retry/<item_id>.part`（相对路径
//! 存队列项）——**不放 `%TEMP%`**（重启后可能被系统清理），确保重试真的
//! 有内容可用。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 最大重试次数（PRD EX-06：≤5 次）。
pub const RETRY_MAX_ATTEMPTS: u32 = 5;

/// 重试队列文件名（位于 data_root 下）。
const QUEUE_FILE: &str = "retry_queue.json";
/// 待安装内容目录（位于 data_root 下）。
const RETRY_DIR: &str = "retry";

/// 队列项（持久化结构）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RetryEntry {
    /// 条目 id。
    pub item_id: String,
    /// 待安装内容相对路径（`retry/<item_id>.part`，相对 data_root）。
    pub rel_part: String,
    /// 已完成的重试次数（0 = 尚未重试）。
    pub attempts: u32,
    /// 下次重试时间（Unix 秒）。
    pub next_at: i64,
    /// 目标文件名（安装到 `repo/<owner>/`）。
    pub file_name: String,
    /// 归属 owner（安装路径）。
    pub owner_id: String,
    /// 期望整体 SHA-256（重试安装前复校验）。
    pub sha256: String,
    /// 入队时间（Unix 秒）。
    pub created_at: i64,
}

/// 重试结果（供 host 生成广播/告警）。
#[derive(Debug, PartialEq, Clone)]
pub enum RetryOutcome {
    /// 安装成功：条目已恢复 `Synced`（内容最新、`.bak` 恰 1 份）。
    Succeeded {
        /// 条目 id。
        item_id: String,
    },
    /// 仍被占用：已退避（attempts+1，未耗尽）。
    StillBusy {
        /// 条目 id。
        item_id: String,
        /// 已重试次数。
        attempts: u32,
        /// 下次重试时间。
        next_at: i64,
    },
    /// 重试耗尽：条目转 `Failed`（retry_count 保持上限值）。
    Exhausted {
        /// 条目 id。
        item_id: String,
    },
    /// 不可重试（内容缺失/哈希不符）：条目转 `Failed`。
    Aborted {
        /// 条目 id。
        item_id: String,
        /// 原因（人类可读）。
        reason: String,
    },
}

/// 判定 IO 错误是否为「目标被占用」（可重试，PRD EX-06）。
///
/// Windows：`ERROR_SHARING_VIOLATION`(32) / `ERROR_ACCESS_DENIED`(5)。
/// 其他平台：`PermissionDenied`（保守视作可重试）。
pub fn is_target_busy(err: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        matches!(err.raw_os_error(), Some(32) | Some(5))
    }
    #[cfg(not(windows))]
    {
        err.kind() == std::io::ErrorKind::PermissionDenied
    }
}

/// 退避间隔（秒）：1、2、4、8、16（第 n 次重试前等待 2^(n-1) 秒）。
pub fn backoff_secs(attempts: u32, initial_secs: i64) -> i64 {
    initial_secs.saturating_mul(1i64 << attempts.min(10))
}

/// 队列持久化文件路径（`data_root/retry_queue.json`）。
pub fn queue_path(root: &Path) -> PathBuf {
    root.join(QUEUE_FILE)
}

/// 待安装内容目录（`data_root/retry/`）。
pub fn retry_dir(root: &Path) -> PathBuf {
    root.join(RETRY_DIR)
}

/// 原子写队列文件：`retry_queue.json.tmp` → rename。
pub fn save_queue(root: &Path, entries: &[RetryEntry]) -> std::io::Result<()> {
    let path = queue_path(root);
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(entries)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::create_dir_all(root)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// 读队列文件：不存在 → 空；损坏/解析失败 → 空 + `warn`（不 panic）。
pub fn load_queue(root: &Path) -> Vec<RetryEntry> {
    let path = queue_path(root);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Vec<RetryEntry>>(&bytes) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "重试队列文件损坏，按空队列继续");
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "重试队列读取失败，按空队列继续");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_exponential_scaled() {
        assert_eq!(backoff_secs(0, 1), 1);
        assert_eq!(backoff_secs(1, 1), 2);
        assert_eq!(backoff_secs(2, 1), 4);
        assert_eq!(backoff_secs(4, 1), 16);
        // 饱和：超大 attempts 不溢出
        assert_eq!(backoff_secs(40, 1), 1i64 << 10);
    }

    #[test]
    fn queue_roundtrip_and_corrupt_file_falls_back_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(load_queue(root).is_empty(), "不存在 → 空队列");
        let entries = vec![RetryEntry {
            item_id: "id-1".into(),
            rel_part: "retry/id-1.part".into(),
            attempts: 2,
            next_at: 1000,
            file_name: "a.txt".into(),
            owner_id: "teacher".into(),
            sha256: "ab".repeat(32),
            created_at: 900,
        }];
        save_queue(root, &entries).unwrap();
        assert_eq!(load_queue(root), entries, "往返一致");
        // 损坏文件 → 空队列（不 panic）
        std::fs::write(queue_path(root), b"{ not valid json").unwrap();
        assert!(load_queue(root).is_empty(), "损坏 → 空队列");
    }
}
