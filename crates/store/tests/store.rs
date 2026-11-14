//! Task 3（store）集成测试：SQLite 索引 + 文件仓库的覆盖、备份、净化与幂等。

use std::fs;
use std::path::{Path, PathBuf};

use classroom_protocol::{
    Audience, Event, EventKind, ItemKind, MergeDecision, NameReject, Role, ShareItem, SyncState,
};
use classroom_store::{
    is_reserved_device_name, sha256_file, ListFilter, PutOutcome, Store, StoreError,
};

/// 建临时仓库根目录（TempDir 保持在测试生命周期内）。
fn root(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join(tag);
    fs::create_dir_all(&root).unwrap();
    (dir, root)
}

/// 在仓库外写一个「已下载/已选中」的临时文件，返回路径。
fn write_tmp(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, bytes).unwrap();
    p
}

/// 构造远端 upsert 事件。
fn remote_upsert(tag: &str, title: &str, version: u64) -> Event {
    Event {
        event_id: format!("ev-{tag}-v{version}"),
        kind: EventKind::Upsert,
        item: ShareItem {
            id: format!("item-{tag}"),
            kind: ItemKind::Text,
            owner_id: "teacher".into(),
            owner_role: Role::Teacher,
            audience: Audience::All,
            title: title.into(),
            size: 2,
            sha256: None,
            rel_path: None,
            created_at: 1000,
            updated_at: 1000 + version as i64,
            version,
            deleted: false,
            sync_state: SyncState::Synced,
        },
        new_name: None,
        acked_by: None,
        ts: 2000,
    }
}

fn dir_entries(dir: &Path) -> Vec<String> {
    match fs::read_dir(dir) {
        Ok(rd) => {
            let mut names: Vec<String> = rd
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
        // 目录不存在视作空（例如 put 被拒时 owner 目录根本不会创建）
        Err(_) => Vec::new(),
    }
}

// ---------- open：布局与幂等 ----------

#[test]
fn open_creates_layout_and_is_idempotent() {
    let (_guard, root) = root("layout");
    {
        let store = Store::open(&root).expect("first open");
        drop(store);
    }
    assert!(root.join("repo").is_dir());
    assert!(root.join("index.db").is_file());
    // 二次打开（schema IF NOT EXISTS）不报错
    Store::open(&root).expect("second open");
}

// ---------- put_text：新建与覆盖（PRD 5.7） ----------

#[test]
fn put_text_first_time_creates() {
    let (_guard, root) = root("text-create");
    let store = Store::open(&root).unwrap();
    let out = store
        .put_text(
            "alice",
            Role::Student,
            Audience::Teacher,
            "笔记",
            "内容",
            100,
        )
        .expect("created");
    match out {
        PutOutcome::Created(item) => {
            assert_eq!(item.version, 1);
            assert_eq!(item.kind, ItemKind::Text);
            assert_eq!(item.size, 2); // 文本 size = 字符数
            let rel = item.rel_path.as_deref().expect("rel_path");
            let body = root.join("repo").join(rel);
            assert_eq!(fs::read_to_string(&body).unwrap(), "内容");
        }
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn put_text_same_scope_overwrites_with_single_bak() {
    let (_guard, root) = root("text-overwrite");
    let store = Store::open(&root).unwrap();
    store
        .put_text(
            "alice",
            Role::Student,
            Audience::Teacher,
            "笔记",
            "旧内容",
            100,
        )
        .unwrap();
    let out = store
        .put_text(
            "alice",
            Role::Student,
            Audience::Teacher,
            "笔记",
            "新内容",
            200,
        )
        .expect("overwrite");
    match out {
        PutOutcome::Overwritten {
            latest,
            old_version,
        } => {
            assert_eq!(old_version, 1);
            assert_eq!(latest.version, 2);
            assert_eq!(latest.updated_at, 200);
        }
        other => panic!("expected Overwritten, got {other:?}"),
    }
    // 文本目录恰 2 个文件：1 份正文 + 1 份 .bak（多版覆盖仍只 1 份备份）
    let texts = root.join("repo/alice/texts");
    let mut names = dir_entries(&texts);
    names.retain(|n| !n.ends_with(".part"));
    assert_eq!(names.len(), 2, "entries: {names:?}");
    assert_eq!(names.iter().filter(|n| n.ends_with(".bak")).count(), 1);
    let bak = texts.join(names.iter().find(|n| n.ends_with(".bak")).unwrap());
    assert_eq!(fs::read_to_string(&bak).unwrap(), "旧内容");
    // 索引中仅一条活条目、正文为新版
    let items = store.list(ListFilter::default()).unwrap();
    assert_eq!(items.len(), 1);
    let body = store.file_body(&items[0]).unwrap();
    assert_eq!(fs::read_to_string(&body).unwrap(), "新内容");
}

// ---------- put_file_from_tmp：两版覆盖恰 1 份 .bak（计划 Step 1） ----------

#[test]
fn file_two_versions_leaves_exactly_one_bak() {
    let (_guard, root) = root("file-versions");
    let store = Store::open(&root).unwrap();

    let v1 = b"first version bytes";
    let tmp1 = write_tmp(root.parent().unwrap(), "a.docx.tmp", v1);
    let h1 = sha256_file(&tmp1).unwrap();
    match store
        .put_file_from_tmp(
            "alice",
            Role::Student,
            Audience::Teacher,
            "作业.docx",
            &tmp1,
            &h1,
            100,
        )
        .unwrap()
    {
        PutOutcome::Created(item) => {
            assert_eq!(item.version, 1);
            assert_eq!(item.sha256.as_deref(), Some(h1.as_str()));
        }
        other => panic!("expected Created, got {other:?}"),
    }

    let v2 = b"second version with more bytes";
    let tmp2 = write_tmp(root.parent().unwrap(), "b.docx.tmp", v2);
    let h2 = sha256_file(&tmp2).unwrap();
    match store
        .put_file_from_tmp(
            "alice",
            Role::Student,
            Audience::Teacher,
            "作业.docx",
            &tmp2,
            &h2,
            200,
        )
        .unwrap()
    {
        PutOutcome::Overwritten {
            latest,
            old_version,
        } => {
            assert_eq!(old_version, 1);
            assert_eq!(latest.version, 2);
            assert_eq!(latest.sha256.as_deref(), Some(h2.as_str()));
        }
        other => panic!("expected Overwritten, got {other:?}"),
    }

    // owner 目录恰 2 个文件：作业.docx + 作业.docx.bak（无 .part 残留）
    let owner_dir = root.join("repo/alice");
    assert_eq!(
        dir_entries(&owner_dir),
        vec!["作业.docx", "作业.docx.bak"],
        "owner dir must contain exactly final + single bak"
    );
    assert_eq!(
        fs::read(owner_dir.join("作业.docx")).unwrap(),
        v2,
        "final is second version"
    );
    assert_eq!(
        fs::read(owner_dir.join("作业.docx.bak")).unwrap(),
        v1,
        "bak keeps first version"
    );
}

#[test]
fn traversal_name_lands_inside_owner_dir() {
    let (_guard, root) = root("traversal");
    let store = Store::open(&root).unwrap();
    let tmp = write_tmp(root.parent().unwrap(), "t.docx", b"payload");
    let h = sha256_file(&tmp).unwrap();
    let out = store
        .put_file_from_tmp(
            "alice",
            Role::Student,
            Audience::Teacher,
            r"..\..\evil.docx",
            &tmp,
            &h,
            100,
        )
        .expect("sanitized into owner dir");
    assert!(matches!(out, PutOutcome::Created(_)));
    // 落在 root/repo/alice/evil.docx，仓库之外与 repo 根均无泄漏
    assert!(root.join("repo/alice/evil.docx").is_file());
    assert!(!root.join("evil.docx").exists());
    assert!(!root.join("repo/evil.docx").exists());
}

#[test]
fn hash_mismatch_rejected_without_final_file() {
    let (_guard, root) = root("hash-mismatch");
    let store = Store::open(&root).unwrap();
    let tmp = write_tmp(root.parent().unwrap(), "h.docx", b"real bytes");
    let wrong = "0".repeat(64);
    let err = store
        .put_file_from_tmp(
            "alice",
            Role::Student,
            Audience::Teacher,
            "a.docx",
            &tmp,
            &wrong,
            100,
        )
        .expect_err("must reject on hash mismatch");
    assert!(matches!(err, StoreError::HashMismatch { .. }));
    let owner_dir = root.join("repo/alice");
    assert!(
        !owner_dir.join("a.docx").exists(),
        "no final file after mismatch"
    );
    assert!(
        !owner_dir.join("a.docx.part").exists(),
        "no .part leftover after mismatch"
    );
}

#[test]
fn risky_extension_rejected() {
    let (_guard, root) = root("risky");
    let store = Store::open(&root).unwrap();
    let tmp = write_tmp(root.parent().unwrap(), "v.exe", b"MZ");
    let h = sha256_file(&tmp).unwrap();
    let err = store
        .put_file_from_tmp(
            "alice",
            Role::Student,
            Audience::Teacher,
            "virus.exe",
            &tmp,
            &h,
            100,
        )
        .expect_err("risky ext must be rejected");
    assert!(matches!(err, StoreError::RiskyExt(_)));
    assert!(!root.join("repo/alice/virus.exe").exists());
}

// ---------- Windows 保留设备名（lead 安全补充项） ----------

#[test]
fn reserved_device_name_detected() {
    assert!(is_reserved_device_name("CON"));
    assert!(is_reserved_device_name("con"));
    assert!(is_reserved_device_name("CON.txt"));
    assert!(is_reserved_device_name("com3.log"));
    assert!(is_reserved_device_name("LPT9"));
    assert!(!is_reserved_device_name("console"));
    assert!(!is_reserved_device_name("报告.pdf"));
    assert!(!is_reserved_device_name("contact.docx"));
}

#[test]
fn reserved_device_name_rejected_at_put() {
    let (_guard, root) = root("reserved");
    let store = Store::open(&root).unwrap();
    let tmp = write_tmp(root.parent().unwrap(), "c.txt", b"text");
    let h = sha256_file(&tmp).unwrap();
    let err = store
        .put_file_from_tmp(
            "alice",
            Role::Student,
            Audience::Teacher,
            "CON.txt",
            &tmp,
            &h,
            100,
        )
        .expect_err("reserved device name must be rejected");
    // PR #28 后由 sanitize 统一拒绝：StoreError::Name(NameReject::ReservedDevice)
    assert!(matches!(err, StoreError::Name(NameReject::ReservedDevice)));
    assert!(dir_entries(&root.join("repo/alice")).is_empty());
}

// ---------- owner_id 防御（PRD 9.4 路径安全） ----------

#[test]
fn bad_owner_id_rejected() {
    let (_guard, root) = root("bad-owner");
    let store = Store::open(&root).unwrap();
    assert!(matches!(
        store.put_text("a/b", Role::Student, Audience::Teacher, "t", "x", 1),
        Err(StoreError::BadOwner(_))
    ));
    assert!(matches!(
        store.put_text("..", Role::Student, Audience::Teacher, "t", "x", 1),
        Err(StoreError::BadOwner(_))
    ));
}

// ---------- list / get / file_body / soft_delete ----------

#[test]
fn list_filter_and_soft_delete_tombstone() {
    let (_guard, root) = root("list");
    let store = Store::open(&root).unwrap();
    let a = store
        .put_text(
            "alice",
            Role::Student,
            Audience::Teacher,
            "数学作业",
            "A",
            1,
        )
        .unwrap();
    let b = store
        .put_text(
            "alice",
            Role::Student,
            Audience::Teacher,
            "英语作业",
            "B",
            2,
        )
        .unwrap();
    let id_a = match &a {
        PutOutcome::Created(i) => i.id.clone(),
        _ => panic!(),
    };
    let _ = b;

    // 远端再来一条（teacher）
    store.apply_remote(&remote_upsert("t1", "通知", 1)).unwrap();

    assert_eq!(store.list(ListFilter::default()).unwrap().len(), 3);
    let f = ListFilter {
        owner: Some("alice".into()),
        ..Default::default()
    };
    assert_eq!(store.list(f.clone()).unwrap().len(), 2);
    let fk = ListFilter {
        owner: Some("alice".into()),
        keyword: Some("数学".into()),
        ..Default::default()
    };
    assert_eq!(store.list(fk).unwrap().len(), 1);
    let fkind = ListFilter {
        kind: Some(ItemKind::Text),
        ..Default::default()
    };
    assert_eq!(store.list(fkind).unwrap().len(), 3);

    // get
    assert!(store.get(&id_a).unwrap().is_some());
    assert!(store.get("no-such").unwrap().is_none());

    // 软删除：墓碑标记，默认列表不可见、include_deleted 可见
    let deleted = store.soft_delete(&id_a, 300).unwrap();
    assert!(deleted.deleted);
    assert_eq!(deleted.sync_state, SyncState::DeleteDeferred);
    assert_eq!(store.list(ListFilter::default()).unwrap().len(), 2);
    let fd = ListFilter {
        include_deleted: true,
        ..Default::default()
    };
    assert_eq!(store.list(fd).unwrap().len(), 3);
    assert!(matches!(
        store.soft_delete("no-such", 1),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn file_body_stays_inside_repo() {
    let (_guard, root) = root("body");
    let store = Store::open(&root).unwrap();
    let tmp = write_tmp(root.parent().unwrap(), "x.pdf", b"%PDF-1");
    let h = sha256_file(&tmp).unwrap();
    let item = match store
        .put_file_from_tmp(
            "alice",
            Role::Student,
            Audience::Teacher,
            "x.pdf",
            &tmp,
            &h,
            1,
        )
        .unwrap()
    {
        PutOutcome::Created(i) => i,
        _ => panic!(),
    };
    let body = store.file_body(&item).unwrap();
    assert!(body.is_absolute());
    assert!(
        body.starts_with(root.join("repo")),
        "body must stay inside repo"
    );
    assert_eq!(fs::read(&body).unwrap(), b"%PDF-1");
}

// ---------- 事件去重与 apply_remote 幂等（PRD 3.6） ----------

#[test]
fn record_and_seen_event() {
    let (_guard, root) = root("events");
    let store = Store::open(&root).unwrap();
    let ev = remote_upsert("x", "通知", 1);
    assert!(!store.seen_event(&ev.event_id).unwrap());
    store.record_event(&ev).unwrap();
    assert!(store.seen_event(&ev.event_id).unwrap());
}

#[test]
fn apply_remote_replay_is_idempotent() {
    let (_guard, root) = root("replay");
    let store = Store::open(&root).unwrap();
    let ev = remote_upsert("r", "通知", 1);
    match store.apply_remote(&ev).unwrap() {
        MergeDecision::Overwrite(_) => {}
        other => panic!("expected Overwrite, got {other:?}"),
    }
    assert_eq!(store.list(ListFilter::default()).unwrap().len(), 1);
    // 同一事件重放：IgnoreDuplicate，索引不变
    assert_eq!(
        store.apply_remote(&ev).unwrap(),
        MergeDecision::IgnoreDuplicate
    );
    assert_eq!(store.list(ListFilter::default()).unwrap().len(), 1);
}

#[test]
fn apply_remote_stale_version_ignored() {
    let (_guard, root) = root("stale");
    let store = Store::open(&root).unwrap();
    store.apply_remote(&remote_upsert("s", "通知", 2)).unwrap();
    let older = remote_upsert("s2", "通知", 1); // 同 title 不同 event_id，version 更旧
    assert_eq!(
        store.apply_remote(&older).unwrap(),
        MergeDecision::IgnoreStale
    );
    let items = store.list(ListFilter::default()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].version, 2);
}
