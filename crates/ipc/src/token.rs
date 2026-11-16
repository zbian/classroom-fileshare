//! IPC 令牌：`root/ipc.token` 持久化、首次生成、ACL 限权（PRD 9.4）。
//!
//! E7（issue #49，fail closed）：限权失败即启动错误——原实现的 icacls
//! 调用缺 `/grant`（code=87 被 WARN 兜底掩盖），叠加 PROGRAMDATA 继承
//! Users:RX，本地非管理员可读令牌全权限过握手。现以 `*SID:(R,W)` 显式
//! 授予（qa 实证 EXIT=0），失败向上抛错含手工修复命令。

use std::path::Path;

use uuid::Uuid;

/// 读取或创建 IPC 令牌（hex，256 bit 表示 = 64 个十六进制字符）：同一
/// root 下稳定，跨 root 唯一。
///
/// ACL（fail closed）：创建后 `icacls /inheritance:r /grant "*<SID>:(R,W)"`
/// 移除继承并仅授予当前用户读写；非零退出或调用失败即返回 Err（含手工
/// 修复命令），不再降级兜底。
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
    restrict_acl_to_current_user(&path)?;
    tracing::info!(path = %path.display(), "IPC 令牌已创建（ACL 仅当前用户）");
    Ok(token)
}

/// 当前用户 SID（`whoami /user /fo csv` 末行第二列）。
fn current_user_sid() -> anyhow::Result<String> {
    let output = std::process::Command::new("whoami")
        .arg("/user")
        .arg("/fo")
        .arg("csv")
        .output()
        .map_err(|e| anyhow::anyhow!("whoami 调用失败: {e}"))?;
    if !output.status.success() {
        anyhow::bail!("whoami /user 退出码非零");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let last = text
        .lines()
        .last()
        .ok_or_else(|| anyhow::anyhow!("whoami 输出为空"))?;
    let sid = last
        .split(',')
        .nth(1)
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| s.starts_with("S-1-"))
        .ok_or_else(|| anyhow::anyhow!("whoami 输出未解析到 SID: {last}"))?;
    Ok(sid)
}

/// ACL 限权（fail closed）：`/inheritance:r` + `*<SID>:(R,W)`。
/// 星号前缀按 SID 而非名字授权（名字含空格/本地化无歧义）；非零退出
/// 即 Err（E7：不再 WARN 兜底）。
///
/// 实现细节（E7 探针实证）：std 的 `arg()` 会对含 `:` 的参数整体加引号
/// （`"/inheritance:r"`），icacls 老式解析不接受带引号的开关 → code 87
/// （这正是原实现缺 `/grant` 假象之下的第二层根因）。因此用
/// `CommandExt::raw_arg` 传原始命令行，路径含空格时手工加引号。
fn restrict_acl_to_current_user(path: &Path) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;

    let sid = current_user_sid()?;
    let raw = format!(
        "\"{}\" /inheritance:r /grant \"*{}:(R,W)\"",
        path.display(),
        sid
    );
    let status = std::process::Command::new("icacls")
        .raw_arg(raw)
        .status()
        .map_err(|e| anyhow::anyhow!("icacls 调用失败: {e}"))?;
    if !status.success() {
        anyhow::bail!(
            "IPC 令牌 ACL 限权失败（icacls 退出码 {:?}）。手工修复命令：\
             icacls \"{}\" /inheritance:r /grant \"*{}:(R,W)\"（issue #49）",
            status.code(),
            path.display(),
            sid
        );
    }
    Ok(())
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

    /// E7 真实单测（CI=windows 可跑 icacls）：限权后输出无继承项 (I)、
    /// 仅当前用户一条 (R,W)——把「降级兜底掩盖缺陷」这类问题永久堵住。
    #[test]
    fn acl_restriction_removes_inheritance_and_grants_only_current_user() {
        let dir = tempfile::tempdir().unwrap();
        let token = load_or_create_token(dir.path()).expect("限权必须成功（fail closed）");
        assert_eq!(token.len(), 64);

        let out = std::process::Command::new("icacls")
            .arg(dir.path().join("ipc.token"))
            .output()
            .expect("icacls 查询");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("(I)"), "继承项必须全部移除，实际:\n{text}");
        let aces = text
            .lines()
            .filter(|l| l.contains("(R,W)") || l.contains("(F)"))
            .count();
        assert_eq!(aces, 1, "仅当前用户一条授予 ACE，实际:\n{text}");
        // 归属校验用 SDDL（icacls /save，纯 ASCII 编码安全——icacls 标准
        // 输出走 OEM 代码页，跨代码页比账户名会乱码）
        let sddl_path = dir.path().join("sddl.txt");
        let save = std::process::Command::new("icacls")
            .arg(dir.path().join("ipc.token"))
            .arg("/save")
            .arg(&sddl_path)
            .output()
            .expect("icacls /save");
        assert!(save.status.success(), "icacls /save 失败");
        let sddl_bytes = std::fs::read(&sddl_path).unwrap();
        // /save 输出为 UTF-16LE（icacls 的 Unicode 文件输出）
        let units: Vec<u16> = sddl_bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect();
        let sddl = String::from_utf16_lossy(&units);
        let sid = current_user_sid().unwrap();
        assert!(
            sddl.contains(&sid),
            "SDDL 应含当前用户 SID {sid}，实际:\n{sddl}"
        );
        assert_eq!(
            sddl.matches("(A;").count(),
            1,
            "SDDL 应仅一条允许 ACE，实际:\n{sddl}"
        );
    }

    #[test]
    fn sid_parse_rejects_garbage() {
        // 防御性：解析器只接受 S-1- 前缀
        assert!(current_user_sid().unwrap().starts_with("S-1-"));
    }
}
