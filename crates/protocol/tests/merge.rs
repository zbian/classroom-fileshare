//! Task 2（protocol）集成测试：幂等合并、作用域键、文本校验、文件名净化、
//! 风险扩展名与事件序列化往返。

use classroom_protocol::{
    is_reserved_device_name, is_risky_ext, merge, sanitize_file_name, validate_text, Audience,
    Event, EventKind, ItemKind, MergeDecision, NameReject, Role, ShareItem, SyncState, TextReject,
    MAX_TEXT_CHARS,
};

/// 构造最小合法 ShareItem（时间戳由测试注入：protocol 不取时钟）。
fn mk_item(owner: &str, audience: Audience, title: &str, version: u64) -> ShareItem {
    ShareItem {
        id: format!("id-{owner}-{title}-v{version}"),
        kind: ItemKind::Text,
        owner_id: owner.to_string(),
        owner_role: Role::Student,
        audience,
        title: title.to_string(),
        size: 3,
        sha256: None,
        rel_path: None,
        created_at: 1_700_000_000,
        updated_at: 1_700_000_000,
        version,
        deleted: false,
        sync_state: SyncState::Synced,
        retry_count: 0,
    }
}

fn mk_event(event_id: &str, item: ShareItem) -> Event {
    Event {
        event_id: event_id.to_string(),
        kind: EventKind::Upsert,
        item,
        new_name: None,
        acked_by: None,
        ts: 1_700_000_001,
    }
}

// ---------- merge：幂等合并决策（PRD 3.6） ----------

#[test]
fn merge_local_none_overwrites() {
    let ev = mk_event("e1", mk_item("alice", Audience::Teacher, "作业", 1));
    match merge(None, &ev) {
        MergeDecision::Overwrite(item) => assert_eq!(*item, ev.item),
        other => panic!("expected Overwrite, got {other:?}"),
    }
}

#[test]
fn merge_higher_version_overwrites() {
    let local = mk_item("alice", Audience::Teacher, "作业", 1);
    let ev = mk_event("e2", mk_item("alice", Audience::Teacher, "作业", 2));
    match merge(Some(&local), &ev) {
        MergeDecision::Overwrite(item) => assert_eq!(item.version, 2),
        other => panic!("expected Overwrite, got {other:?}"),
    }
}

#[test]
fn merge_stale_version_ignored() {
    let local = mk_item("alice", Audience::Teacher, "作业", 3);
    let ev = mk_event("e3", mk_item("alice", Audience::Teacher, "作业", 2));
    assert_eq!(merge(Some(&local), &ev), MergeDecision::IgnoreStale);
}

#[test]
fn merge_replayed_event_is_duplicate() {
    // 同一事件（同 event_id、同 version）重放：忽略，不产生第二条记录。
    let local = mk_item("alice", Audience::Teacher, "作业", 2);
    let ev = mk_event("e2", mk_item("alice", Audience::Teacher, "作业", 2));
    assert_eq!(merge(Some(&local), &ev), MergeDecision::IgnoreDuplicate);
}

#[test]
fn merge_same_version_different_event_id_is_duplicate() {
    let local = mk_item("alice", Audience::Teacher, "作业", 2);
    let ev = mk_event("other-id", mk_item("alice", Audience::Teacher, "作业", 2));
    assert_eq!(merge(Some(&local), &ev), MergeDecision::IgnoreDuplicate);
}

// ---------- 同版本 delete 合并（接口修订 #5①，PRD 7.4 删除同步） ----------

#[test]
fn merge_delete_same_version_overwrites_live_local() {
    // 教师删除不加 version；删除意图必须覆盖同版本的未删本地条目，
    // 否则删除永远无法在学员端落地。
    let mut local = mk_item("teacher", Audience::All, "课件", 1);
    local.deleted = false;
    let mut gone = local.clone();
    gone.deleted = true;
    let ev = Event {
        kind: EventKind::Delete,
        item: gone.clone(),
        ..mk_event("e-del", local.clone())
    };
    assert_eq!(
        merge(Some(&local), &ev),
        MergeDecision::Overwrite(Box::new(gone))
    );
}

#[test]
fn merge_delete_on_already_deleted_local_is_duplicate() {
    // 重复 delete（本地已是墓碑）：幂等忽略
    let mut local = mk_item("teacher", Audience::All, "课件", 1);
    local.deleted = true;
    let mut gone = local.clone();
    gone.deleted = true;
    let ev = Event {
        kind: EventKind::Delete,
        item: gone,
        ..mk_event("e-del", local.clone())
    };
    assert_eq!(merge(Some(&local), &ev), MergeDecision::IgnoreDuplicate);
}

// ---------- scope_key：同名覆盖判定（PRD 5.7） ----------

#[test]
fn scope_key_equal_for_same_tuple() {
    let a = mk_item("alice", Audience::Teacher, "作业.docx", 1);
    let b = mk_item("alice", Audience::Teacher, "作业.docx", 2);
    assert_eq!(a.scope_key(), b.scope_key());
}

#[test]
fn scope_key_differs_by_owner() {
    // 同名不同 owner 不会互相覆盖（学生同名文件隔离）。
    let a = mk_item("alice", Audience::Teacher, "作业.docx", 1);
    let b = mk_item("bob", Audience::Teacher, "作业.docx", 1);
    assert_ne!(a.scope_key(), b.scope_key());
}

#[test]
fn scope_key_differs_by_audience() {
    // 同 owner 不同接收范围不互相覆盖。
    let a = mk_item("teacher", Audience::All, "通知", 1);
    let b = mk_item("teacher", Audience::Teacher, "通知", 1);
    assert_ne!(a.scope_key(), b.scope_key());
}

// ---------- validate_text（PRD 5.2 / 9.4） ----------

#[test]
fn validate_text_accepts_normal() {
    assert_eq!(validate_text("通知", "明天带教材"), Ok(()));
}

#[test]
fn validate_text_rejects_blank_title() {
    assert!(matches!(
        validate_text("  ", "x"),
        Err(TextReject::BlankTitle)
    ));
}

#[test]
fn validate_text_rejects_blank_body() {
    assert!(matches!(
        validate_text("标题", "   "),
        Err(TextReject::BlankBody)
    ));
}

#[test]
fn validate_text_rejects_over_limit() {
    let body = "字".repeat(MAX_TEXT_CHARS + 1);
    assert!(matches!(
        validate_text("标题", &body),
        Err(TextReject::TooLong(n)) if n == MAX_TEXT_CHARS + 1
    ));
}

#[test]
fn validate_text_accepts_at_limit() {
    let body = "字".repeat(MAX_TEXT_CHARS);
    assert_eq!(validate_text("标题", &body), Ok(()));
}

// ---------- sanitize_file_name（PRD 9.4） ----------

#[test]
fn sanitize_strips_path_segments() {
    assert_eq!(sanitize_file_name("a/b.txt"), Ok("b.txt".to_string()));
    assert_eq!(
        sanitize_file_name(r"..\..\evil.exe"),
        Ok("evil.exe".to_string())
    );
    assert_eq!(
        sanitize_file_name(r"C:\tmp\报告.pdf"),
        Ok("报告.pdf".to_string())
    );
}

#[test]
fn sanitize_rejects_illegal_chars() {
    assert!(matches!(
        sanitize_file_name("报告<>.pdf"),
        Err(NameReject::IllegalChar)
    ));
}

#[test]
fn sanitize_rejects_traversal_only_names() {
    assert!(matches!(
        sanitize_file_name(".."),
        Err(NameReject::Traversal)
    ));
    assert!(matches!(
        sanitize_file_name("a/.."),
        Err(NameReject::Traversal)
    ));
    assert!(matches!(
        sanitize_file_name("."),
        Err(NameReject::Traversal)
    ));
}

#[test]
fn sanitize_rejects_empty() {
    assert!(matches!(sanitize_file_name(""), Err(NameReject::Empty)));
    assert!(matches!(sanitize_file_name(r"a\"), Err(NameReject::Empty)));
}

// ---------- is_risky_ext（PRD 5.3） ----------

#[test]
fn risky_ext_detected_case_insensitive() {
    assert!(is_risky_ext("x.lnk"));
    assert!(is_risky_ext("Photo.EXE"));
    assert!(is_risky_ext("run.exe"));
    // 大小写不影响判定方向：JPG 不在风险清单，不因大写而误拦。
    assert!(!is_risky_ext("Photo.JPG"));
    assert!(!is_risky_ext("课件.docx"));
    assert!(!is_risky_ext("noext"));
}

// ---------- PR #28 安全修复：尾随点/空格与 NTFS 保留设备名 ----------
//
// 绕过链：sanitize("x.exe.") 原放行（尾随点不在非法字符集）→ is_risky_ext
// 取到空扩展不拦 → Win32 落盘剥尾随点/空格 → 磁盘上实际是 x.exe。

#[test]
fn sanitize_rejects_trailing_dot() {
    assert!(matches!(
        sanitize_file_name("x.exe."),
        Err(NameReject::TrailingDotOrSpace)
    ));
}

#[test]
fn sanitize_rejects_trailing_space() {
    assert!(matches!(
        sanitize_file_name("x.exe "),
        Err(NameReject::TrailingDotOrSpace)
    ));
    assert!(matches!(
        sanitize_file_name("课件.pdf "),
        Err(NameReject::TrailingDotOrSpace)
    ));
    // 点+空格混合尾随同样拒绝
    assert!(matches!(
        sanitize_file_name("x.exe. "),
        Err(NameReject::TrailingDotOrSpace)
    ));
}

#[test]
fn risky_ext_normalizes_trailing_dot_and_space() {
    // 防调用方绕过 sanitize 直接调用：入口归一后判定
    assert!(is_risky_ext("x.exe."));
    assert!(is_risky_ext("x.exe "));
    assert!(is_risky_ext("x.EXE."));
    assert!(!is_risky_ext("报告.pdf."));
}

#[test]
fn sanitize_rejects_reserved_device_names() {
    assert!(matches!(
        sanitize_file_name("CON"),
        Err(NameReject::ReservedDevice)
    ));
    assert!(matches!(
        sanitize_file_name("con.txt"),
        Err(NameReject::ReservedDevice)
    ));
    // 剥路径后是保留名同样拒绝
    assert!(matches!(
        sanitize_file_name(r"..\..\COM1"),
        Err(NameReject::ReservedDevice)
    ));
    assert!(is_reserved_device_name("NUL.txt"));
    assert!(is_reserved_device_name("lpt9"));
    assert!(!is_reserved_device_name("console"));
    assert!(!is_reserved_device_name("contact.docx"));
}

// ---------- 事件序列化往返 ----------

#[test]
fn event_json_roundtrip() {
    let ev = Event {
        event_id: "e-9".to_string(),
        kind: EventKind::Rename,
        item: mk_item("teacher", Audience::All, "课件.pptx", 4),
        new_name: Some("课件-v2.pptx".to_string()),
        acked_by: Some("alice".to_string()),
        ts: 1_700_000_009,
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(back.event_id, ev.event_id);
    assert!(matches!(back.kind, EventKind::Rename));
    assert_eq!(back.item, ev.item);
    assert_eq!(back.new_name.as_deref(), Some("课件-v2.pptx"));
    assert_eq!(back.acked_by.as_deref(), Some("alice"));
    assert_eq!(back.ts, ev.ts);
}
