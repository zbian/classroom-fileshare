//! IPC 令牌：`root/ipc.token` 持久化、首次生成、ACL 限权（PRD 9.4）。

use std::path::Path;

use uuid::Uuid;

/// 读取或创建 IPC 令牌（hex，128 bit 随机）：同一 root 下稳定，跨 root 唯一。
///
/// ACL：创建后尝试用 `icacls` 移除继承并仅授予当前用户读写（纵深防御；
/// 管道握手本身仍校验 token）。限权失败不阻断——告警并继续，由握手兜底。
pub fn load_or_create_token(root: &Path) -> anyhow::Result<String> {
    let path = root.join("ipc.token");
    if path.exists() {
        let token = std::fs::read_to_string(&path)?;
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    std::fs::create_dir_all(root)?;
    std::fs::write(&path, &token)?;
    restrict_acl_to_current_user(&path);
    tracing::info!(path = %path.display(), "IPC 令牌已创建");
    Ok(token)
}

/// 尽力而为的 ACL 限权：`/inheritance:r` + 当前用户 RW。
fn restrict_acl_to_current_user(path: &Path) {
    let Some(user) = std::env::var("USERNAME").ok().filter(|u| !u.is_empty()) else {
        tracing::warn!("无法获取用户名，跳过 IPC 令牌 ACL 限权");
        return;
    };
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg(format!("{user}:(R,W)"))
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => tracing::warn!(code = s.code(), "icacls 限权返回非零，由握手校验兜底"),
        Err(e) => tracing::warn!(error = %e, "icacls 调用失败，由握手校验兜底"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_stable_within_same_root_and_unique_across_roots() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let t1 = load_or_create_token(a.path()).unwrap();
        let t2 = load_or_create_token(a.path()).unwrap();
        let t3 = load_or_create_token(b.path()).unwrap();
        assert_eq!(t1, t2, "同 root 读取稳定");
        assert_ne!(t1, t3, "不同 root 唯一");
        assert_eq!(t1.len(), 64, "两个 uuid v4 simple 拼接 = 64 hex");
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()), "纯 hex");
    }
}
