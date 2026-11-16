//! PR #38 发现 5 冒烟：两个独立连接（handler store 与核心 store 并发
//! 场景）同时写同一 index.db——busy_timeout 下不应出现 SQLITE_BUSY
//! 瞬时报错（并发线程全部成功落库且互不覆盖）。

use classroom_protocol::{Audience, Role};
use classroom_store::{ListFilter, PutOutcome, Store};

#[test]
fn concurrent_writes_from_two_connections_all_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let handles: Vec<_> = (0..2)
        .map(|i| {
            let root = root.clone();
            std::thread::spawn(move || {
                let store = Store::open(&root).expect("open");
                for n in 0..25 {
                    let outcome = store
                        .put_text(
                            "teacher",
                            Role::Teacher,
                            Audience::All,
                            &format!("条目-{i}-{n}"),
                            &format!("正文 {i} {n}"),
                            1_000 + n as i64,
                        )
                        .expect("并发写不应报 SQLITE_BUSY");
                    assert!(matches!(outcome, PutOutcome::Created(_)));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("写线程成功");
    }

    // 全部落库：50 条互不覆盖
    let store = Store::open(&root).unwrap();
    let all = store.list(ListFilter::default()).unwrap();
    assert_eq!(all.len(), 50, "两连接并发写的 50 条应全部在库");

    // 修订 #8③：WAL 生效（journal_mode 持久化在 db 文件）
    let wal: String = store
        .pragma_query("journal_mode")
        .expect("journal_mode 查询");
    assert_eq!(wal, "wal", "WAL 应持久生效");
}
