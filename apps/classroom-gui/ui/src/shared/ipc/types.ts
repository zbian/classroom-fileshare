/**
 * ipc 类型镜像 —— 与 crates/ipc（feat/t7-ipc）线协议逐字段对齐。
 * 字段为 serde 默认 snake_case（crate 未启用 rename_all）；接线时如有调整以此处同步。
 * 依据：crates/ipc/src/lib.rs（GuiRequest/SvcNotify/ItemUpdatedPayload/GuiReply）
 * 与计划 Task 2/4 的 ShareItem/Beacon 形态。
 */

export type ItemKind = 'Text' | 'File'
export type Audience = 'All' | 'Teacher'
export type Role = 'Teacher' | 'Student'
/** SyncState：Synced 已同步 …RetryLocked 被占用重试（PRD 7.6 状态字典） */
export type SyncState = 'Synced' | 'PendingSend' | 'PendingSync' | 'DeleteDeferred' | 'RetryLocked' | 'Failed'

export interface ShareItem {
  id: string
  kind: ItemKind
  owner_id: string
  owner_role: Role
  audience: Audience
  title: string
  size: number
  sha256?: string | null
  rel_path?: string | null
  created_at: number
  updated_at: number
  version: number
  deleted: boolean
  sync_state: SyncState
  /** 占用退避重试次数（#103 契约：0 表示未进入重试；上限 5 次后转 Failed） */
  retry_count?: number
}

export interface ListFilter {
  owner?: string | null
  audience?: Audience | null
  kind?: ItemKind | null
  keyword?: string | null
  include_deleted?: boolean
}

export interface Beacon {
  protocol_version: number
  room: string
  tcp_port: number
  pin_required: boolean
  host_name: string
}

/** SvcNotify::ItemUpdated 载荷（修订 #4：E3 覆盖提示需要 old_version） */
export interface ItemUpdatedPayload {
  item: ShareItem
  overwritten: boolean
  old_version?: number | null
}

/// 学员提交分组（#94 契约）：教师端「学生提交」左树行
/// 口径：audience=Teacher 条目按 owner 聚合（不含墓碑），latest_at 为 Unix 秒（最大 updated_at）
export interface SubmissionGroup {
  owner_id: string
  count: number
  latest_at: number
}

export type GuiRequest =
  | { t: 'List'; filter: ListFilter }
  | { t: 'SendText'; title: string; body: string }
  | { t: 'SendFiles'; paths: string[] }
  | { t: 'DeleteItem'; id: string }
  | { t: 'OpenItem'; id: string }
  | { t: 'SetNickname'; nickname: string; pin?: string | null }
  | { t: 'ConfigureRoom'; room: string; pin?: string | null; allow_discovery: boolean }
  | { t: 'Status' }
  | { t: 'Submissions' }
  | { t: 'PauseTips'; paused: boolean }
  | { t: 'ShutdownGuiOnly' }

/** #103 告警类型（与 Rust WarningKind 对齐，snake 序列化为 CamelCase 值） */
export type WarningKind = 'BackupFailed' | 'RetryExhausted' | 'DiskFull'

// 注意：服务端 notify 经 Rust `retag()` 统一产出 `{"t": 变体名, "payload": 载荷}` 外壳
// （ItemRemoved 载荷为 {"id"}、Discovered 为 {"beacons":[…]}、StatusChanged 为裸字符串）。
// 前端一律读 `n.payload`；读成 `n.item` / `n.id` / `n.beacons` 会静默失效
// （M2 #20b 修复的 3 处同类 retag bug：学生提交联动 / 删除即时移除 / B1 房间发现列表）。
export type SvcNotify =
  | { t: 'ShareReceived'; payload: ShareItem }
  | { t: 'ItemUpdated'; payload: ItemUpdatedPayload }
  | { t: 'ItemRemoved'; payload: { id: string } }
  | { t: 'StatusChanged'; payload: string }
  | { t: 'TransferProgress'; payload: { item_id: string; pct: number } }
  | { t: 'Discovered'; payload: { beacons: Beacon[] } }
  /** #103：同步异常告警（占用重试耗尽/备份写入失败/磁盘不足）；图形化聚合展示 */
  | {
      t: 'Warning'
      payload: { kind: WarningKind; item_id: string; detail: string }
    }

export type GuiReply =
  | { t: 'Ok' }
  | { t: 'Items'; items: ShareItem[] }
  /** 修订 #6④：结构化状态快照（替代原 StatusText 单轨） */
  | { t: 'Status'; snapshot: StatusSnapshot }
  /// #94：教师端学生提交分组（retag 后契约键 submissions）
  | { t: 'Submissions'; submissions: SubmissionGroup[] }
  /** 修订 #7①：OpenItem 返回实体绝对路径（GUI 用户会话负责打开） */
  | { t: 'FilePath'; path: string }

/** 服务运行状态（ipc ServiceState，serde lowercase） */
export type ServiceState = 'running' | 'stopped' | 'unreachable'

/** 服务状态快照（crates/ipc StatusSnapshot，修订 #6④） */
export interface StatusSnapshot {
  service_state: ServiceState
  room: string
  online_count: number
  nickname: string
  role: 'Teacher' | 'Student'
  pending_count: number
}

/** UI 侧解构后的服务状态（StatusText 为人类可读文本，真实接线时建议后端结构化，见 T9 报告） */
export interface ServiceStatus {
  running: boolean
  room: string
  online: number
  raw: string
}
