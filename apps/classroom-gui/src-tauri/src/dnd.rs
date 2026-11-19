//! HTML5 拖放落盘：WebView 拿不到绝对路径，前端把字节以 base64 传来，
//! 这里校验、落临时文件、走既有 SendFiles 链路，并在发送后清理临时目录。
//!
//! 评审修复（rc.10）：
//! - B1：拖放态窗口放大改由 `set_icon_drop_state` 命令驱动（release 可用）
//! - M1：每个文件独立子目录，文件名保持原始（不再加 `0-` 前缀，避免同名覆盖失效）
//! - M2：base64 长度/数量/总量前置校验（服务端 100 MB 校验之前先拒绝）
//! - M3：发送结束（成功或失败）删除临时目录；启动时清理历史残留
//! - M4：目录名含原子自增，先全部解码再统一写盘，失败回滚

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use classroom_ipc::GuiRequest;
use classroom_protocol::MAX_FILE_SIZE;

use crate::ipc_client;

/// 单次拖放文件数量上限。
pub const MAX_DROP_FILES: usize = 50;
/// 单次拖放总大小上限（与前端 dnd.ts 保持一致）。
pub const MAX_DROP_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
const TEMP_ROOT: &str = "ClassShareDrop";

static DROP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DroppedFileDto {
    pub name: String,
    pub data_base64: String,
}

/// 文件名净化：复用协议层实现（剥路径/拒绝非法字符/保留设备名）。
/// 净化失败时替换非法字符后重试，仍失败则退回 `dropped`。
pub fn sanitize_name(raw: &str) -> String {
    if let Ok(name) = classroom_protocol::sanitize_file_name(raw) {
        return name;
    }
    let replaced: String = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    classroom_protocol::sanitize_file_name(&replaced).unwrap_or_else(|_| "dropped".into())
}

/// base64 解码后的精确字节数（含 `=` padding 修正，避免 100MB 边界误拒）。
pub fn decoded_len(encoded: &str) -> u64 {
    let len = encoded.len() as u64;
    let pad = encoded.chars().rev().take_while(|c| *c == '=').count() as u64;
    (len / 4) * 3 - pad
}

/// 拖放前置校验：数量、单文件、总量。返回错误文案，Ok 表示可继续。
pub fn validate(files: &[DroppedFileDto]) -> Result<(), String> {
    if files.len() > MAX_DROP_FILES {
        return Err(format!("单次最多拖放 {MAX_DROP_FILES} 个文件"));
    }
    let mut total = 0u64;
    for f in files {
        let size = decoded_len(&f.data_base64);
        if size > MAX_FILE_SIZE {
            return Err(format!(
                "「{}」超过单文件 100 MB 上限，请改用「添加文件」或压缩后发送",
                sanitize_name(&f.name)
            ));
        }
        total += size;
    }
    if total > MAX_DROP_TOTAL_BYTES {
        return Err("单次拖放总大小超过 200 MB，请分批发送".into());
    }
    Ok(())
}

pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(text.as_bytes())
        .map_err(|e| format!("拖放文件解码失败: {e}"))
}

fn temp_root() -> PathBuf {
    std::env::temp_dir().join(TEMP_ROOT)
}

fn unique_dir_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = DROP_SEQ.fetch_add(1, Ordering::SeqCst);
    format!("{nanos}-{}-{seq}", std::process::id())
}

/// 发送时的 TTL：超过 10 分钟的旧目录才清理，避免删掉服务端仍在读取的在途文件。
const TEMP_TTL_SECS: u64 = 10 * 60;
/// 启动时 TTL：只清 1 小时前的残留（进程崩溃/升级遗留）。
const STARTUP_TTL_SECS: u64 = 60 * 60;

fn is_stale(now: std::time::SystemTime, modified: std::time::SystemTime, ttl_secs: u64) -> bool {
    now.duration_since(modified)
        .map(|d| d.as_secs() > ttl_secs)
        .unwrap_or(false)
}

/// 清理超过 `max_age_secs` 的拖放临时目录。
///
/// 不在 send 应答后立即删：服务端 `SendFiles` 应答仅表示命令已入队
/// （runtime.rs 的 `try_send`），host/agent 之后才读取文件；立即删会让
/// 多文件拖放的后几个文件读取失败且前端假成功。改为 TTL 清理。
pub fn cleanup_stale(max_age_secs: u64) {
    let root = temp_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if is_stale(now, modified, max_age_secs) {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// 启动时清理历史拖放临时目录（只清 1 小时前的残留，避免误删在途文件）。
pub fn cleanup_root() {
    cleanup_stale(STARTUP_TTL_SECS);
}

/// 落盘并通过既有 IPC 链路发文件。
///
/// 不在这里立刻删除临时目录：服务端 `SendFiles` 应答只代表命令已入队
///（runtime 的 `try_send`），host/agent 随后才读取文件；立即删会造成
/// 多文件拖放的后几个文件读取失败（且前端误报成功）。改为 TTL 清理。
pub async fn save_and_send(
    app: tauri::AppHandle,
    files: Vec<DroppedFileDto>,
) -> Result<usize, String> {
    if files.is_empty() {
        return Ok(0);
    }
    validate(&files)?;
    // 发送前顺带清掉 10 分钟前的旧目录（含历史失败残留）
    cleanup_stale(TEMP_TTL_SECS);

    // 先全部解码：任一失败则不落盘（避免部分成功）
    let mut decoded: Vec<(String, Vec<u8>)> = Vec::with_capacity(files.len());
    for f in &files {
        decoded.push((sanitize_name(&f.name), decode_base64(&f.data_base64)?));
    }

    let dir = temp_root().join(unique_dir_name());
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {e}"))?;

    // M1：每个文件独立子目录，文件名保持原名（不污染同名覆盖语义）
    let write_result = (|| -> Result<Vec<PathBuf>, String> {
        let mut paths = Vec::with_capacity(decoded.len());
        for (i, (name, bytes)) in decoded.iter().enumerate() {
            let sub = dir.join(i.to_string());
            std::fs::create_dir_all(&sub).map_err(|e| format!("创建临时子目录失败: {e}"))?;
            let path = sub.join(name);
            std::fs::write(&path, bytes).map_err(|e| format!("写入拖放文件失败: {e}"))?;
            paths.push(path);
        }
        Ok(paths)
    })();

    let paths = match write_result {
        Ok(paths) => paths,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };

    let count = paths.len();
    let send = ipc_client::svc_request(app, GuiRequest::SendFiles { paths }).await;
    // 失败也保留文件：TTL 清理会兜底，便于用户重试时文件仍在
    send?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto(name: &str, encoded: &str) -> DroppedFileDto {
        DroppedFileDto {
            name: name.into(),
            data_base64: encoded.into(),
        }
    }

    #[test]
    fn sanitize_strips_path_and_illegal_chars() {
        assert_eq!(sanitize_name("..\\evil.exe"), "evil.exe");
        assert_eq!(sanitize_name("C:\\a\\b.txt"), "b.txt");
        assert_eq!(sanitize_name("a:b?.txt"), "a_b_.txt");
        assert_eq!(sanitize_name("..."), "dropped");
    }

    #[test]
    fn decode_rejects_garbage_and_accepts_valid() {
        assert!(decode_base64("!!!not-base64!!!").is_err());
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
    }

    #[test]
    fn decoded_len_handles_padding() {
        assert_eq!(decoded_len("AAAA"), 3);
        assert_eq!(decoded_len("aGVsbG8="), 5);
        assert_eq!(decoded_len("aGVsbG8h"), 6);
    }

    #[test]
    fn decoded_len_accepts_exact_100mb_boundary() {
        // 100MB = 104857600B，base64 长度 139810136，末尾补 2 个 '='
        let encoded_len = ((MAX_FILE_SIZE as f64 / 3.0).ceil() * 4.0) as usize;
        let encoded = format!("{}{}", "A".repeat(encoded_len - 2), "==");
        assert_eq!(decoded_len(&encoded), MAX_FILE_SIZE);
        let files = vec![dto("edge.bin", &encoded)];
        assert!(validate(&files).is_ok(), "恰好 100MB 不应被拒绝");
    }

    #[test]
    fn stale_detection_honours_ttl() {
        let now = std::time::SystemTime::now();
        let old = now - std::time::Duration::from_secs(3600);
        let fresh = now - std::time::Duration::from_secs(10);
        assert!(is_stale(now, old, 600));
        assert!(!is_stale(now, fresh, 600));
    }

    #[test]
    fn validate_rejects_too_many_files() {
        let files: Vec<_> = (0..=MAX_DROP_FILES).map(|_| dto("a.txt", "")).collect();
        assert!(validate(&files).is_err());
    }

    #[test]
    fn validate_accepts_small_batch() {
        let files = vec![dto("a.txt", "aGVsbG8="), dto("b.txt", "aGVsbG8=")];
        assert!(validate(&files).is_ok());
    }

    #[test]
    fn unique_dir_names_differ() {
        assert_ne!(unique_dir_name(), unique_dir_name());
    }
}
