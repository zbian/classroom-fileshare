-- 课堂共享索引 schema v1（PRD 3.5 数据模型）
-- 由 Store::open 幂等执行；版本号记录在 PRAGMA user_version。

CREATE TABLE IF NOT EXISTS share_items (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,          -- Text | File
    owner_id   TEXT NOT NULL,
    owner_role TEXT NOT NULL,          -- Teacher | Student
    audience   TEXT NOT NULL,          -- All | Teacher
    title      TEXT NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT,
    rel_path   TEXT,                   -- 相对 root/repo/ 的路径
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version    INTEGER NOT NULL,
    deleted    INTEGER NOT NULL DEFAULT 0,
    sync_state TEXT NOT NULL           -- SyncState 枚举名
);

-- 同名覆盖作用域键（PRD 5.7）：owner|audience|名称，仅对未删除条目唯一
CREATE UNIQUE INDEX IF NOT EXISTS ux_share_items_scope
    ON share_items (owner_id, audience, title)
    WHERE deleted = 0;

CREATE INDEX IF NOT EXISTS ix_share_items_owner ON share_items (owner_id);

-- 事件去重表（PRD 3.6：按 event_id 去重）
CREATE TABLE IF NOT EXISTS event_log (
    event_id TEXT PRIMARY KEY,
    ts       INTEGER NOT NULL
);
