//! IPC 令牌：`root/ipc.token` 持久化、首次生成、ACL 限权（PRD 9.4）。
//!
//! E7（issue #49，fail closed）：限权失败即启动错误——原实现的 icacls
//! 调用缺 `/grant`（code=87 被 WARN 兜底掩盖），叠加 PROGRAMDATA 继承
//! Users:RX，本地非管理员可读令牌全权限过握手。现以 `*SID:(R,W)` 显式
//! 授予（qa 实证 EXIT=0），失败向上抛错含手工修复命令。

use std::path::Path;

use uuid::Uuid;

/// 读取或创建 IPC 令牌（hex，256 bit 表示 = 64 个十六进制字符）：同一
/// root 下稳定，跨 root 唯一。仅授予当前用户（console/agent 模式：
/// 数据根本就用户私有）。
///
/// ACL（fail closed）：`icacls /inheritance:r /grant "*<SID>:(R,W)"`；
/// 非零退出或调用失败即返回 Err（含手工修复命令），不再降级兜底。
pub fn load_or_create_token(root: &Path) -> anyhow::Result<String> {
    load_or_create_token_for(root, None)
}

/// 服务模式变体（E7 追加，令牌分发机制）：`user_sid` 为主登录用户 SID
/// （install.ps1 写入 svc.json）——限权为「LocalSystem（创建者隐含）+
/// 双授予 `*<user_sid>:(R,W)`」，用户会话 GUI 才能读到令牌过握手。
/// `None` 时仅 LocalSystem 可读，GUI 不可用属配置缺失（调用方告警）。
pub fn load_or_create_token_for(root: &Path, user_sid: Option<&str>) -> anyhow::Result<String> {
    let path = root.join("ipc.token");
    let restrict = |p: &Path| restrict_acl(p, user_sid);
    if path.exists() {
        let token = std::fs::read_to_string(&path)?;
        let token = token.trim().to_string();
        if !token.is_empty() {
            // E7 追加（qa 复验⑤）：存量机器（旧版安装）的令牌可能带
            // 继承 ACL——幂等重限权实现自愈升级；失败同样 fail closed
            restrict(&path)?;
            return Ok(token);
        }
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    std::fs::create_dir_all(root)?;
    std::fs::write(&path, &token)?;
    restrict(&path)?;
    tracing::info!(
        path = %path.display(),
        user_sid = ?user_sid,
        "IPC 令牌已创建（ACL：LocalSystem + 主登录用户）"
    );
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

/// 令牌分发（E7 追加，会话自愈）：服务收到登录会话事件后对既有令牌
/// 重授 `*<sid>:(R,W)`（幂等，LocalSystem 授予保留）。M1 单元形态：
/// 直接调用即覆盖 WTS 集成路径（真机由 SessionChange 触发）。
///
/// 防 ACE 累积（终审 nit 2）：授予前先 `/remove:g "*<sid>"`——同 SID
/// 重复登录不会逐条累积。实测：可解析 SID 无论有无既有授予均退出 0
/// （幂等安全）；不可解析 SID 返回 1332，仅记 warn 不阻断（授予为
/// fail closed 主闸）。M2 备注：不同用户交替登录时旧会话 SID 的 ACE
/// 由下一次 restrict 全量重建覆盖。
pub fn grant_for_session(root: &Path, sid: &str) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;

    if !sid.starts_with("S-1-") {
        anyhow::bail!("非法 SID: {sid}");
    }
    let path = root.join("ipc.token");
    let remove_raw = format!("\"{}\" /remove:g \"*{sid}\"", path.display());
    let remove_status = std::process::Command::new("icacls")
        .raw_arg(remove_raw)
        .status()
        .map_err(|e| anyhow::anyhow!("icacls 调用失败: {e}"))?;
    if !remove_status.success() {
        tracing::warn!(code = remove_status.code(), %sid, "移除既有授予非零（授予步骤仍执行）");
    }
    restrict_acl(&path, Some(sid))
}

/// ACL 限权（fail closed）：`/inheritance:r` + 按序授予各 SID（当前用户
/// 必含；服务模式追加主登录用户 `user_sid`——令牌分发机制，E7 追加）。
/// 星号前缀按 SID 而非名字授权（名字含空格/本地化无歧义）；非零退出
/// 即 Err（E7：不再 WARN 兜底）。
///
/// 实现细节（E7 探针实证）：std 的 `arg()` 会对含 `:` 的参数整体加引号
/// （`"/inheritance:r"`），icacls 老式解析不接受带引号的开关 → code 87
/// （这正是原实现缺 `/grant` 假象之下的第二层根因）。因此用
/// `CommandExt::raw_arg` 传原始命令行，路径含空格时手工加引号。
fn restrict_acl(path: &Path, user_sid: Option<&str>) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;

    let sid = current_user_sid()?;
    let mut grants = vec![format!("*{sid}:(R,W)")];
    if let Some(us) = user_sid.filter(|s| s.starts_with("S-1-")) {
        grants.push(format!("*{us}:(R,W)"));
    }
    let raw = format!(
        "\"{}\" /inheritance:r /grant {}",
        path.display(),
        grants.join(" ")
    );
    let status = std::process::Command::new("icacls")
        .raw_arg(raw)
        .status()
        .map_err(|e| anyhow::anyhow!("icacls 调用失败: {e}"))?;
    if !status.success() {
        anyhow::bail!(
            "IPC 令牌 ACL 限权失败（icacls 退出码 {:?}）。手工修复命令：\
             icacls \"{}\" /inheritance:r /grant {}（issue #49）",
            status.code(),
            path.display(),
            grants.join(" ")
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
    fn sid_parse_accepts_s1_prefix() {
        // 终审 nit：断言接受 S-1- 前缀（原名 rejects_garbage 与断言相反）
        assert!(current_user_sid().unwrap().starts_with("S-1-"));
    }

    /// E7 追加（qa 复验⑤）：存量机器（旧版安装）预置带继承+Users 可读的
    /// 脆弱 ACL 令牌 → load 幂等重限权自愈（无 (I)、仅当前用户 RW）。
    #[test]
    fn load_upgrades_legacy_weak_acl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ipc.token");
        std::fs::write(
            &path,
            format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
        )
        .unwrap();
        // 模拟旧版遗留（真实场景：PROGRAMDATA 父目录继承 Users:RX）——
        // 父目录授 Everyone 可读 + 文件 /reset 重算继承，文件即带继承 ACE
        use std::os::windows::process::CommandExt;
        let st = std::process::Command::new("icacls")
            .raw_arg(format!(
                "\"{}\" /grant \"*S-1-1-0:(OI)(CI)(R)\"",
                dir.path().display()
            )) // S-1-1-0 = Everyone，可向子对象继承
            .status()
            .unwrap();
        assert!(st.success(), "父目录预置失败");
        let st = std::process::Command::new("icacls")
            .raw_arg(format!("\"{}\" /reset", path.display()))
            .status()
            .unwrap();
        assert!(st.success(), "文件继承重算失败");

        // load（existing 分支）必须重限权
        let token = load_or_create_token(dir.path()).expect("存量令牌自愈必须成功");
        assert_eq!(token.len(), 64);

        // 断言 ACL 已重限权：无 Everyone、无 (I)、仅当前用户一条授予
        let out = std::process::Command::new("icacls")
            .arg(&path)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("(I)"), "继承必须移除，实际:\n{text}");
        assert!(
            !text.contains("Everyone") && !text.contains("S-1-1-0"),
            "Everyone 可读必须被清除，实际:\n{text}"
        );
        let aces = text
            .lines()
            .filter(|l| l.contains("(R,W)") || l.contains("(F)") || l.contains("(R)"))
            .count();
        assert_eq!(aces, 1, "应仅剩当前用户一条授予 ACE，实际:\n{text}");

        // SDDL 归属校验（编码安全）
        let sddl_path = dir.path().join("sddl.txt");
        let save = std::process::Command::new("icacls")
            .arg(&path)
            .arg("/save")
            .arg(&sddl_path)
            .output()
            .unwrap();
        assert!(save.status.success());
        let sddl_bytes = std::fs::read(&sddl_path).unwrap();
        let sddl = String::from_utf16_lossy(
            &sddl_bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        );
        assert!(sddl.contains(&current_user_sid().unwrap()), "SDDL:\n{sddl}");
    }

    /// 令牌分发机制（E7 追加）：user_sid 存在 → 双授予（SDDL 两条允许
    /// ACE：LocalSystem 路径的当前用户 + 主登录用户）。
    #[test]
    fn dual_grant_includes_user_sid() {
        let dir = tempfile::tempdir().unwrap();
        let users_sid = "S-1-5-32-545"; // BUILTIN\Users（真实可解析；icacls /save 输出知名 SID 的确定性别名 "BU"）
        let token =
            load_or_create_token_for(dir.path(), Some(users_sid)).expect("双授予限权必须成功");
        assert_eq!(token.len(), 64);

        let sddl_path = dir.path().join("sddl.txt");
        let save = std::process::Command::new("icacls")
            .arg(dir.path().join("ipc.token"))
            .arg("/save")
            .arg(&sddl_path)
            .output()
            .unwrap();
        assert!(save.status.success());
        let sddl_bytes = std::fs::read(&sddl_path).unwrap();
        let sddl = String::from_utf16_lossy(
            &sddl_bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        );
        assert!(
            sddl.contains(&current_user_sid().unwrap()),
            "应含当前用户：\n{sddl}"
        );
        // 知名 SID 在 /save 输出中确定性别名化（Users → "BU"）
        assert!(
            sddl.contains("(A;;0x12019f;;;BU)"),
            "应含主登录用户授予：\n{sddl}"
        );
        assert_eq!(
            sddl.matches("(A;").count(),
            2,
            "双授予应为两条允许 ACE，实际:\n{sddl}"
        );

        // None：仅当前用户（配置缺失语义，svc 侧负责告警）
        let dir2 = tempfile::tempdir().unwrap();
        load_or_create_token_for(dir2.path(), None).unwrap();
        let sddl2_path = dir2.path().join("sddl.txt");
        let save2 = std::process::Command::new("icacls")
            .arg(dir2.path().join("ipc.token"))
            .arg("/save")
            .arg(&sddl2_path)
            .output()
            .unwrap();
        assert!(save2.status.success());
        let sddl2_bytes = std::fs::read(&sddl2_path).unwrap();
        let sddl2 = String::from_utf16_lossy(
            &sddl2_bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            sddl2.matches("(A;").count(),
            1,
            "无 user_sid 应仅一条允许 ACE，实际:\n{sddl2}"
        );
    }
}
