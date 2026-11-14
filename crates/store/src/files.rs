//! 文件仓库辅助：SHA-256 计算与 hex 编码。
//!
//! 原子替换说明（PRD 7.3）：`std::fs::rename` 在 Windows 上底层即
//! `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`——先 rename、目标存在则原子覆盖，
//! 计划所述「rename 失败退 MoveFileEx REPLACE_EXISTING」的语义已由标准库一步满足；
//! 若 rename 仍失败（典型为目标被独占打开），向上返回 IO 错误，由上层进入
//! 占用重试队列（PRD EX-06）。

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// 计算文件 SHA-256（流式读取），返回小写 hex。
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

/// 字节转小写 hex。
pub fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}
