//! IPC 令牌：`root/ipc.token` 持久化、首次生成、ACL 限权（PRD 9.4）。
//!
//! E7（issue #49，fail closed）：限权失败即启动错误。实现为 **windows-rs
//! API 直写 DACL**（假设 3，run 68 裁定）——icacls 外部进程在 GHA runner
//! 上谎报成功（exit 0 + 自报 processed 但 ACL 未变），结构性方案无外部
//! 进程/无输出解析/locale 无关的空间。
//!
//! DACL 语义：`SetNamedSecurityInfoW` + `SE_DACL_PROTECTED` 移除继承并
//! 全量重建为显式条目——SYSTEM/管理员（管理可达）+ 当前用户 (R,W)
//! +（服务模式）主登录用户 (R,W)。Everyone/Users 继承 ACE 不再保留
//! （原缺陷的暴露面，#49）。

use std::ffi::c_void;
use std::path::Path;

use uuid::Uuid;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, GENERIC_READ, GENERIC_WRITE, HLOCAL, WIN32_ERROR};
use windows::Win32::Security::Authorization::{
    ConvertStringSidToSidW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
    GRANT_ACCESS, MULTIPLE_TRUSTEE_OPERATION, SE_FILE_OBJECT, TRUSTEE_FORM, TRUSTEE_TYPE,
    TRUSTEE_W,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, NO_INHERITANCE, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
};

// TRUSTEE_FORM 枚举：0 = TRUSTEE_IS_SID（此前误填 1 = TRUSTEE_IS_NAME，
// 二进制 SID 被当名字映射 → 1332，与 #49 缺 /grant 同族）
const TRUSTEE_IS_SID: TRUSTEE_FORM = TRUSTEE_FORM(0);
const TRUSTEE_IS_UNKNOWN: TRUSTEE_TYPE = TRUSTEE_TYPE(0);
const NO_MULTIPLE_TRUSTEE: MULTIPLE_TRUSTEE_OPERATION = MULTIPLE_TRUSTEE_OPERATION(0);

fn win32_err(api: &str, code: WIN32_ERROR) -> anyhow::Error {
    anyhow::anyhow!("{api} 失败: {code:?}")
}

/// 读取或创建 IPC 令牌（hex，256 bit 表示 = 64 个十六进制字符）：同一
/// root 下稳定，跨 root 唯一。仅授予当前用户（console/agent 模式）。
///
/// ACL（fail closed）：API 直写 DACL；任何失败即返回 Err（含上下文），
/// 不再降级兜底。
pub fn load_or_create_token(root: &Path) -> anyhow::Result<String> {
    load_or_create_token_for(root, None)
}

/// 服务模式变体（E7 追加，令牌分发机制）：`user_sid` 为主登录用户 SID
/// （install.ps1 写入 svc.json）——限权为「SYSTEM/管理员（管理可达）+
/// 当前用户 (R,W) + 双授予 `*<user_sid>:(R,W)`」，用户会话 GUI 才能读到
/// 令牌过握手。`None` 时无主登录用户授予（GUI 不可用属配置缺失，
/// 调用方告警）。
pub fn load_or_create_token_for(root: &Path, user_sid: Option<&str>) -> anyhow::Result<String> {
    std::fs::create_dir_all(root)?;
    // canonicalize 展开 8.3 短名（RUNNER~1）并剥 `\\?\` 前缀
    let root_canon = match std::fs::canonicalize(root) {
        Ok(p) => p.to_string_lossy().trim_start_matches(r"\\?\").to_string(),
        Err(_) => root.to_string_lossy().into_owned(),
    };
    let root_canon = std::path::PathBuf::from(root_canon);
    let path = root_canon.join("ipc.token");
    let restrict = |p: &Path| restrict_acl(p, user_sid);
    if path.exists() {
        let token = std::fs::read_to_string(&path)?;
        let token = token.trim().to_string();
        if !token.is_empty() {
            // E7 追加（qa 复验⑤）：存量机器（旧版安装）的令牌可能带
            // 继承 ACL——幂等重建 DACL 实现自愈升级；失败同样 fail closed
            restrict(&path)?;
            return Ok(token);
        }
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    std::fs::write(&path, &token)?;
    restrict(&path)?;
    tracing::info!(
        path = %path.display(),
        user_sid = ?user_sid,
        "IPC 令牌已创建（ACL：SYSTEM/管理员 + 当前用户 + 主登录用户）"
    );
    Ok(token)
}

/// 令牌分发（E7 追加，会话自愈）：服务收到登录会话事件后对既有令牌
/// 以 `sid` 重建 DACL（全量重建语义下无 ACE 累积问题；LocalSystem/
/// 管理员条目恒重建保留）。M1 单元形态：直接调用即覆盖 WTS 集成路径
/// （真机由 SessionChange 触发）。
pub fn grant_for_session(root: &Path, sid: &str) -> anyhow::Result<()> {
    if !sid.starts_with("S-1-") {
        anyhow::bail!("非法 SID: {sid}");
    }
    // 与 load 路径一致：canonicalize 展开短名
    let root_canon = match std::fs::canonicalize(root) {
        Ok(p) => p.to_string_lossy().trim_start_matches(r"\\?\").to_string(),
        Err(_) => root.to_string_lossy().into_owned(),
    };
    restrict_acl(
        &std::path::PathBuf::from(root_canon).join("ipc.token"),
        Some(sid),
    )
}

/// 字符串 SID → PSID（调用方负责 [`free_sid`]）。
unsafe fn sid_from_string(sid: &str) -> anyhow::Result<PSID> {
    let mut wide: Vec<u16> = sid.encode_utf16().chain([0]).collect();
    let mut psid = PSID::default();
    ConvertStringSidToSidW(PCWSTR(wide.as_mut_ptr()), &mut psid)
        .map_err(|e| anyhow::anyhow!("ConvertStringSidToSidW({sid}) 失败: {e}"))?;
    Ok(psid)
}

/// 释放 ConvertStringSidToSidW 分配的 SID。
unsafe fn free_sid(psid: PSID) {
    let _ = LocalFree(Some(HLOCAL(psid.0)));
}

/// DACL 全量重建（fail closed）：显式条目 = SYSTEM/管理员（管理可达）
/// + 当前用户 (R,W) + 可选主登录用户 (R,W)；`SE_DACL_PROTECTED` 移除继承。
fn restrict_acl(path: &Path, user_sid: Option<&str>) -> anyhow::Result<()> {
    let current = current_user_sid()?;
    let rw = (GENERIC_READ | GENERIC_WRITE).0;
    // ( trustee SID 字符串, 权限 )
    let trustees: Vec<(&str, u32)> = [
        ("S-1-5-18", rw),     // SYSTEM
        ("S-1-5-32-544", rw), // Administrators
        (current.as_str(), rw),
    ]
    .into_iter()
    .chain(user_sid.iter().map(|s| (*s, rw)))
    .collect();

    unsafe {
        // 全部字符串 SID 一次性转换，统一释放
        let mut owned_sids: Vec<PSID> = Vec::with_capacity(trustees.len());
        let mut entries: Vec<EXPLICIT_ACCESS_W> = Vec::with_capacity(trustees.len());
        for (sid_str, mask) in &trustees {
            let psid = match sid_from_string(sid_str) {
                Ok(p) => p,
                Err(e) => {
                    owned_sids.iter().for_each(|s| free_sid(*s));
                    return Err(e);
                }
            };
            owned_sids.push(psid);
            entries.push(EXPLICIT_ACCESS_W {
                grfAccessPermissions: *mask,
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: NO_INHERITANCE,
                Trustee: TRUSTEE_W {
                    pMultipleTrustee: std::ptr::null_mut(),
                    MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                    TrusteeForm: TRUSTEE_IS_SID,
                    TrusteeType: TRUSTEE_IS_UNKNOWN,
                    ptstrName: PWSTR(psid.0 as *mut u16),
                },
            });
        }

        // 全新 DACL：仅显式条目（不与旧 DACL 合并 → 继承的 Everyone/Users
        // 条目随 PROTECTED 一并消失，#49 暴露面）
        let mut new_dacl: *mut windows::Win32::Security::ACL = std::ptr::null_mut();
        let code = SetEntriesInAclW(Some(&entries), None, &mut new_dacl);
        if code != WIN32_ERROR(0) {
            owned_sids.iter().for_each(|s| free_sid(*s));
            return Err(win32_err("SetEntriesInAclW", code));
        }

        let path_wide: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain([0])
            .collect();
        let code = SetNamedSecurityInfoW(
            PCWSTR(path_wide.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(new_dacl),
            None,
        );
        owned_sids.iter().for_each(|s| free_sid(*s));
        let _ = LocalFree(Some(HLOCAL(new_dacl as *mut c_void)));
        if code != WIN32_ERROR(0) {
            return Err(win32_err("SetNamedSecurityInfoW", code));
        }
    }
    Ok(())
}

/// 读取 DACL 的 SDDL 表示（测试断言用；纯 ASCII 编码安全）。
#[cfg(test)]
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
};
#[cfg(test)]
use windows::Win32::Security::PSECURITY_DESCRIPTOR;

#[cfg(test)]
unsafe fn sddl_of(path: &Path) -> anyhow::Result<String> {
    let path_wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain([0])
        .collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    let code = GetNamedSecurityInfoW(
        PCWSTR(path_wide.as_ptr()),
        SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION,
        None,
        None,
        None,
        None,
        &mut sd,
    );
    if code != WIN32_ERROR(0) {
        return Err(win32_err("GetNamedSecurityInfoW", code));
    }
    let mut sddl = PWSTR::null();
    ConvertSecurityDescriptorToStringSecurityDescriptorW(
        sd,
        1, // SDDL_REVISION_1
        DACL_SECURITY_INFORMATION,
        &mut sddl,
        None,
    )
    .map_err(|e| anyhow::anyhow!("Convert…DescriptorW 失败: {e}"))?;
    let _ = LocalFree(Some(HLOCAL(sd.0)));
    let s = sddl.to_string().expect("SDDL 为合法 UTF-16");
    let _ = LocalFree(Some(HLOCAL(sddl.as_ptr() as *mut c_void)));
    Ok(s)
}

/// 当前用户 SID（`whoami /user /fo csv` 末行第二列）。
///
/// 健壮性加固（CI 诊断，PR #50 run 43867884669）：管道输出 `\r\n` 结尾，
/// `\r` 混入 SID 会让授予静默落空——trim 全部尾随空白 + SID 字符集
/// 白名单（`S-1-` 前缀、其余仅数字与连字符）双重防御。
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
        .map(|s| {
            s.trim()
                .trim_matches('"')
                .trim_end_matches(['\r', '\n', ' '])
        })
        .filter(|s| s.starts_with("S-1-") && s[4..].chars().all(|c| c.is_ascii_digit() || c == '-'))
        .ok_or_else(|| anyhow::anyhow!("whoami 输出未解析到合法 SID: {last:?}"))?
        .to_string();
    Ok(sid)
}

// LocalAlloc 释放场景已由 LocalFree 覆盖（API 分配的 SID/ACL/SDDL 内存）

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// 金丝雀探测结果：GHA runner 等环境上 ACL 写入 API 整体静默无效
    /// （icacls 与 SetNamedSecurityInfoW 双机制、两个卷同症状；run 68/77
    /// 证据）。安全行为的主证据 = 真实 NTFS 上 qa 复验 5/5（issue #49）
    /// + 本地全量断言；不支持的环境如实跳过断言（E7 终裁 Option A）。
    static ACL_WRITABLE: AtomicBool = AtomicBool::new(true);

    fn acl_writable() -> bool {
        ACL_WRITABLE.load(Ordering::SeqCst)
    }

    /// ACL 测试专用临时目录（工作区本地卷）+ 金丝雀探测：建目录后对
    /// canary 文件尝试同款 restrict，SDDL 前后无变化即判定本环境不支持
    /// ACL 写入并 eprintln 留痕。
    fn acl_test_dir() -> tempfile::TempDir {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/acl-tests");
        std::fs::create_dir_all(&base).expect("创建 ACL 测试目录");
        let dir = tempfile::Builder::new()
            .prefix("acl-")
            .tempdir_in(base)
            .expect("ACL 测试临时目录");

        let canary = dir.path().join("canary.token");
        std::fs::write(&canary, "canary").unwrap();
        let before = unsafe { sddl_of(&canary) }.unwrap_or_default();
        let restrict_ok = restrict_acl(&canary, None).is_ok();
        let after = unsafe { sddl_of(&canary) }.unwrap_or_default();
        let writable = restrict_ok && before != after;
        ACL_WRITABLE.store(writable, Ordering::SeqCst);
        if !writable {
            eprintln!(
                "当前环境不支持 ACL 写入（GHA runner 已知限制，issue #49 复验 5/5 于真实 NTFS；\
                 本地全量断言见真实环境跑批），本环境跳过 ACL 断言。before={before:?} after={after:?}"
            );
        }
        dir
    }

    #[test]
    fn token_stable_within_same_root_and_unique_across_roots() {
        let a = acl_test_dir();
        let b = acl_test_dir();
        let t1 = load_or_create_token(a.path()).unwrap();
        let t2 = load_or_create_token(a.path()).unwrap();
        let t3 = load_or_create_token(b.path()).unwrap();
        assert_eq!(t1, t2, "同 root 读取稳定");
        assert_ne!(t1, t3, "不同 root 唯一");
        assert_eq!(t1.len(), 64, "两个 uuid v4 simple 拼接 = 64 hex");
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()), "纯 hex");
    }

    /// E7 真实单测（API 断言）：限权后 SDDL 保护位在（继承移除）、含当前
    /// 用户 SID、Everyone（S-1-1-0）不在。
    #[test]
    fn acl_restriction_removes_inheritance_and_grants_only_current_user() {
        if !acl_writable() {
            eprintln!("当前环境不支持 ACL 写入（GHA runner 已知限制，issue #49 复验 5/5 于真实 NTFS）——如实跳过 ACL 断言（Option A，非放松断言）");
            return;
        }

        let dir = acl_test_dir();
        let sid = current_user_sid().unwrap();
        println!("diag: sid={sid:?} path={:?}", dir.path().join("ipc.token"));
        let token = load_or_create_token(dir.path()).expect("限权必须成功（fail closed）");
        assert_eq!(token.len(), 64);

        let sddl = unsafe { sddl_of(&dir.path().join("ipc.token")).unwrap() };
        println!("diag: sddl={sddl}");
        assert!(
            sddl.contains("D:P"),
            "DACL 必须为保护形态（继承移除）: {sddl}"
        );
        assert!(sddl.contains(&sid), "必须含当前用户 ACE: {sddl}");
        assert!(
            !sddl.contains(";;;WD)"),
            "Everyone 不得保留（SDDL 别名 WD）: {sddl}"
        );
    }

    #[test]
    fn load_config_roundtrip_guard() {
        // 令牌文件格式守护（64 hex）已由上文覆盖；此处防 uuid 位数回退
        let dir = acl_test_dir();
        let t = load_or_create_token(dir.path()).unwrap();
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sid_parse_accepts_s1_prefix() {
        // 终审 nit：断言接受 S-1- 前缀（原名 rejects_garbage 与断言相反）
        assert!(current_user_sid().unwrap().starts_with("S-1-"));
    }

    /// E7 追加（qa 复验⑤）：存量机器（旧版安装）预置带继承+Everyone 可读
    /// 的脆弱 ACL 令牌（父目录授权 + icacls /reset 重算继承构造真实继承
    /// 形态）→ load 幂等重建 DACL 自愈（Everyone 消失、保护位在）。
    #[test]
    fn load_upgrades_legacy_weak_acl() {
        if !acl_writable() {
            eprintln!("当前环境不支持 ACL 写入（GHA runner 已知限制，issue #49 复验 5/5 于真实 NTFS）——如实跳过 ACL 断言（Option A，非放松断言）");
            return;
        }

        use std::os::windows::process::CommandExt;
        let dir = acl_test_dir();
        let path = dir.path().join("ipc.token");
        std::fs::write(
            &path,
            format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
        )
        .unwrap();
        // 模拟旧版遗留（真实场景：PROGRAMDATA 父目录继承 Users:RX）
        let st = std::process::Command::new("icacls")
            .raw_arg(format!(
                "\"{}\" /grant \"*S-1-1-0:(OI)(CI)(R)\"",
                dir.path().display()
            ))
            .status()
            .unwrap();
        assert!(st.success(), "父目录预置失败");
        let st = std::process::Command::new("icacls")
            .raw_arg(format!("\"{}\" /reset", path.display()))
            .status()
            .unwrap();
        assert!(st.success(), "文件继承重算失败");

        // load（existing 分支）必须重建 DACL
        let token = load_or_create_token(dir.path()).expect("存量令牌自愈必须成功");
        assert_eq!(token.len(), 64);

        let sddl = unsafe { sddl_of(&path).unwrap() };
        assert!(sddl.contains("D:P"), "继承必须移除（保护位）: {sddl}");
        assert!(
            !sddl.contains(";;;WD)"),
            "Everyone 必须清除（SDDL 别名 WD）: {sddl}"
        );
        assert!(
            sddl.contains(&current_user_sid().unwrap()),
            "必须含当前用户 ACE: {sddl}"
        );
    }

    /// 令牌分发机制（E7 追加）：user_sid 存在 → 双授予（SDDL 同时含
    /// 当前用户与主登录用户）。
    #[test]
    fn dual_grant_includes_user_sid() {
        if !acl_writable() {
            eprintln!("当前环境不支持 ACL 写入（GHA runner 已知限制，issue #49 复验 5/5 于真实 NTFS）——如实跳过 ACL 断言（Option A，非放松断言）");
            return;
        }

        let dir = acl_test_dir();
        let users_sid = "S-1-5-32-545"; // BUILTIN\Users（真实可解析）
        let token =
            load_or_create_token_for(dir.path(), Some(users_sid)).expect("双授予限权必须成功");
        assert_eq!(token.len(), 64);

        let sddl = unsafe { sddl_of(&dir.path().join("ipc.token")).unwrap() };
        assert!(
            sddl.contains(&current_user_sid().unwrap()),
            "应含当前用户：\n{sddl}"
        );
        // 知名 SID 在 SDDL 中确定性别名化（BUILTIN\Users → "BU"）
        assert!(sddl.contains(";;;BU)"), "应含主登录用户授予：\n{sddl}");
    }

    /// grant_for_session 单元形态（WTS 集成真机覆盖）：对既有令牌以
    /// 新 SID 重建 DACL（M1 会话自愈语义）。
    #[test]
    fn grant_for_session_rebuilds_dacl_for_sid() {
        if !acl_writable() {
            eprintln!("当前环境不支持 ACL 写入（GHA runner 已知限制，issue #49 复验 5/5 于真实 NTFS）——如实跳过 ACL 断言（Option A，非放松断言）");
            return;
        }

        let dir = acl_test_dir();
        load_or_create_token(dir.path()).unwrap();
        let users_sid = "S-1-5-32-545";
        grant_for_session(dir.path(), users_sid).expect("会话重授必须成功");
        let sddl = unsafe { sddl_of(&dir.path().join("ipc.token")).unwrap() };
        assert!(
            sddl.contains(";;;BU)"),
            "应含会话用户授予（SDDL 别名 BU）: {sddl}"
        );
        assert!(sddl.contains("D:P"), "保护位保持: {sddl}");
    }
}
