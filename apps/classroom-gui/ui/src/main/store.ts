/**
 * 主界面状态（zustand）。T9 教师视图 + T10 学员视图/首次昵称（mock 驱动）。
 * ipc notify → applyNotify；请求失败统一落入 serviceDown（教师 EX-10 / 学员 EX-08）。
 */
import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { getClient, type GuiClient, type LocalRole, type UiShareItem } from '../shared/ipc/client'
import type { ListFilter, ServiceStatus, SubmissionGroup, SvcNotify } from '../shared/ipc/types'
import { EMPTY_FILTER, type KindFilter, type TabFilter } from './listFilter'

export type TabKey = 'mine' | 'submissions' | 'records' | 'teacher'
export type Role = LocalRole

export interface DialogState {
  pasteText: boolean
  createRoom: boolean
  nickname: boolean
  deleteConfirm: string | null // 待删除条目 id
}

interface MainStore {
  client: GuiClient
  role: Role
  /** 学员已完成首次昵称（B1 关闭条件） */
  nicknameDone: boolean
  /** B1 自动发现的教师机房间（SvcNotify::Discovered 消费；TC-01 步骤 4） */
  discovered: { room: string; host: string; pinRequired: boolean }[]
  items: UiShareItem[]
  status: ServiceStatus | null
  serviceDown: boolean
  activeTab: TabKey
  selectedId: string | null
  /// 当前 Tab 的搜索关键字（镜像 tabFilters[activeTab]）
  keyword: string
  /// 当前 Tab 的类型筛选（镜像 tabFilters[activeTab]）
  kindFilter: KindFilter
  /// 按 Tab 独立保存的筛选状态（切换 Tab 保留各自筛选，issue #90）
  tabFilters: Record<TabKey, TabFilter>
  /** 已查看条目（D1「新」标记消除，客户端态） */
  seenIds: string[]
  /** #16：教师端「学生提交」分组（左树：昵称/条数/最新时间；latest_at 倒序） */
  groups: SubmissionGroup[]
  /** 当前选中的学员昵称（右表过滤键） */
  selectedOwner: string | null
  /** 选中学员的提交条目（只读展示；来源 ListFilter{audience:Teacher, owner}） */
  submissionItems: UiShareItem[]
  /** #16：本地已读水位（owner → 已读到的 latest_at）。后端无未读字段，前端本地维护 */
  seenOwners: Record<string, number>
  dialog: DialogState
  pasteBody: string
  toast: string | null
  // 动作
  init(): Promise<void>
  refresh(): Promise<void>
  retryService(): Promise<void>
  setActiveTab(t: TabKey): void
  select(id: string | null): void
  markSeen(id: string): void
  /** #16：拉取学员提交分组（左树），默认选中最新提交的学员并加载其右表 */
  loadSubmissions(): Promise<void>
  /** #16：选中某学员（右表加载 + 本地已读水位推进） */
  selectOwner(owner: string): Promise<void>
  /** #16：加载某学员的提交条目（只读） */
  loadSubmissionItems(owner: string): Promise<void>
  setKeyword(kw: string): void
  setKindFilter(k: 'all' | 'Text' | 'File'): void
  openDialog<K extends keyof DialogState>(key: K, id?: string): void
  closeDialog(key: keyof DialogState): void
  sendText(title: string, body: string): Promise<boolean>
  confirmDelete(): Promise<void>
  configureRoom(room: string, pin: string | null, allowDiscovery: boolean): Promise<boolean>
  setNickname(nickname: string, pin?: string | null): Promise<string | null>
  setPasteBody(b: string): void
  showToast(msg: string): void
  applyNotify(n: SvcNotify): void
}

/** 学员提交本地已读水位的存储键（#16：后端无未读字段，GUI 本地维护） */
const SUBMISSION_SEEN_KEY = 'classshare.submissions.seen'

/** 学员当前 Tab 对应的列表过滤 */
function studentFilter(tab: TabKey): ListFilter {
  // 学员数据范围由 Tab 决定（教师共享 / 我共享的）；搜索与类型筛选在客户端完成
  return { owner: tab === 'teacher' ? 'teacher' : null }
}

export const useMainStore = create<MainStore>((set, get) => ({
  client: getClient(),
  role: 'unset',
  nicknameDone: false,
  discovered: [],
  items: [],
  status: null,
  serviceDown: false,
  activeTab: 'mine',
  selectedId: null,
  keyword: '',
  kindFilter: 'all',
  tabFilters: {
    mine: { ...EMPTY_FILTER },
    teacher: { ...EMPTY_FILTER },
    // 预留：#16 B（学员发送 UI）/后续搜索接线前不使用；先保留与 Tab 对齐的结构
    submissions: { ...EMPTY_FILTER },
    records: { ...EMPTY_FILTER },
  },
  seenIds: [],
  groups: [],
  selectedOwner: null,
  submissionItems: [],
  seenOwners: {},
  dialog: { pasteText: false, createRoom: false, nickname: false, deleteConfirm: null },
  pasteBody: '',
  toast: null,

  async init() {
    const { client } = get()
    client.subscribe((n) => get().applyNotify(n))
    void import('@tauri-apps/api/event').then(async ({ listen }) => {
      await listen<string>('gui://paste-text', (e) => {
        get().setPasteBody(e.payload)
        get().openDialog('pasteText')
      })
    })
    // 角色来自 gui.json（Rust get_icon_state；M1 由 --role= 写入，产品期由服务/安装器决定）
    try {
      const st = await client.status().catch(() => null)
      const iconState = await invoke<{ pausedTips: boolean; nicknameDone?: boolean; role: string }>(
        'get_icon_state',
      ).catch(() => null)
      const role: Role =
        iconState?.role === 'student' ? 'student' : iconState?.role === 'teacher' ? 'teacher' : 'unset'
      client.setContext?.({ role: role === 'unset' ? 'teacher' : role })
      const nicknameDone = iconState?.nicknameDone === true
      set({ role, status: st, nicknameDone })
      // B1 仅首次弹（PRD 6.1：昵称本地持久化，重启不再弹）
      if (role === 'student' && !nicknameDone) set({ dialog: { ...get().dialog, nickname: true } })
    } catch {
      set({ role: 'teacher' })
    }
    // 学生提交：本地已读水位（后端无未读字段；#16 约定 GUI 本地维护，跨重启保留）
    try {
      const raw = localStorage.getItem(SUBMISSION_SEEN_KEY)
      if (raw) set({ seenOwners: JSON.parse(raw) as Record<string, number> })
    } catch {
      // localStorage 不可用（隐私模式/配额）时忽略：仅未读高亮失效，不影响功能
    }
    await get().refresh()
  },

  async refresh() {
    const { client, role, activeTab } = get()
    // #16：「学生提交」Tab 数据源是分组聚合（Submissions），不走 List
    if (activeTab === 'submissions' && role === 'teacher') {
      await get().loadSubmissions()
      return
    }
    // 一次拉全量，搜索/类型筛选在客户端完成（即时、无 IPC 抖动；issue #90）
    const filter: ListFilter =
      role === 'student'
        ? { ...studentFilter(activeTab), include_deleted: false }
        : { include_deleted: false } // wire 契约：bool 无 serde default，必须显式携带
    try {
      const [items, status] = await Promise.all([client.list(filter), client.status()])
      set({ items, status, serviceDown: !status.running })
    } catch {
      // 教师 EX-10 / 学员 EX-08：服务未运行/未连接 —— 列表置空并提示重试
      set({ serviceDown: true, items: [] })
    }
  },

  async retryService() {
    await get().refresh()
  },

  setActiveTab(activeTab) {
    // 恢复该 Tab 自己的筛选（issue #90：筛选状态按 Tab 独立）
    const filter = get().tabFilters[activeTab]
    set({ activeTab, selectedId: null, keyword: filter.keyword, kindFilter: filter.kind })
    if (activeTab === 'submissions') {
      // #16：进入「学生提交」→ 拉分组并默认选中最新学员
      void get().loadSubmissions()
    } else {
      void get().refresh()
    }
  },
  select(selectedId) {
    set({ selectedId })
    if (selectedId) get().markSeen(selectedId)
  },
  markSeen(id) {
    if (!get().seenIds.includes(id)) set({ seenIds: [...get().seenIds, id] })
  },

  /** #16：拉取分组；保留当前选中（仍存在时），否则默认最新提交的学员 */
  async loadSubmissions() {
    try {
      const groups = await get().client.submissions()
      const current = get().selectedOwner
      const owner =
        current && groups.some((g) => g.owner_id === current)
          ? current
          : (groups[0]?.owner_id ?? null)
      set({ groups })
      if (owner) {
        // 默认选中同样视为「已查看」：右表已展示其内容，未读点不应停留
        await get().selectOwner(owner)
      } else {
        set({ selectedOwner: null, submissionItems: [] })
      }
    } catch {
      set({ groups: [], submissionItems: [], serviceDown: true })
    }
  },

  /** #16：选中学生 → 加载其提交（只读）+ 推进本地已读水位 */
  async selectOwner(owner) {
    set({ selectedOwner: owner, selectedId: null })
    const group = get().groups.find((g) => g.owner_id === owner)
    if (group) {
      const seenOwners = { ...get().seenOwners, [owner]: group.latest_at }
      set({ seenOwners })
      try {
        localStorage.setItem(SUBMISSION_SEEN_KEY, JSON.stringify(seenOwners))
      } catch {
        // 本地持久化失败不影响本次会话的高亮
      }
    }
    await get().loadSubmissionItems(owner)
  },

  /** #16：契约口径——条目内容经 List 取，audience=Teacher + owner=昵称 */
  async loadSubmissionItems(owner) {
    try {
      const items = await get().client.list({
        audience: 'Teacher',
        owner,
        include_deleted: false,
      })
      set({ submissionItems: items })
    } catch {
      set({ submissionItems: [] })
    }
  },
  setKeyword(keyword) {
    const activeTab = get().activeTab
    set({
      keyword,
      tabFilters: { ...get().tabFilters, [activeTab]: { ...get().tabFilters[activeTab], keyword } },
    })
    // 客户端过滤：不重新拉取（避免逐字符 IPC；issue #90）
  },
  setKindFilter(kindFilter) {
    const activeTab = get().activeTab
    set({
      kindFilter,
      tabFilters: { ...get().tabFilters, [activeTab]: { ...get().tabFilters[activeTab], kind: kindFilter } },
    })
  },

  openDialog(key, id) {
    if (key === 'deleteConfirm') set({ dialog: { ...get().dialog, deleteConfirm: id ?? null } })
    else set({ dialog: { ...get().dialog, [key]: true } })
  },
  closeDialog(key) {
    if (key === 'deleteConfirm') set({ dialog: { ...get().dialog, deleteConfirm: null } })
    else set({ dialog: { ...get().dialog, [key]: false } })
  },

  async sendText(title, body) {
    try {
      await get().client.sendText(title, body)
      set({ dialog: { ...get().dialog, pasteText: false } })
      get().showToast(get().role === 'student' ? '已发送给教师' : '已共享给全体学员')
      await get().refresh()
      return true
    } catch (e) {
      get().showToast(e instanceof Error ? e.message : '发送失败')
      return false
    }
  },

  async confirmDelete() {
    const id = get().dialog.deleteConfirm
    if (!id) return
    try {
      await get().client.deleteItem(id)
      set({ dialog: { ...get().dialog, deleteConfirm: null }, selectedId: null })
      // PRD 5.8 同步进度：逐台 N/M 需后端提供离线端/完成端计数（#103），
      // 此处仅用真实字段 online_count，不臆造进度。
      const online = get().status?.online ?? 0
      get().showToast(`已删除：在线 ${online} 台学员端已同步移除；离线端上线后自动删除`)
      await get().refresh()
    } catch (e) {
      get().showToast(e instanceof Error ? e.message : '删除失败')
    }
  },

  async configureRoom(room, pin, allowDiscovery) {
    try {
      await get().client.configureRoom(room, pin, allowDiscovery)
      set({ dialog: { ...get().dialog, createRoom: false } })
      await get().refresh()
      return true
    } catch (e) {
      get().showToast(e instanceof Error ? e.message : '开课失败')
      return false
    }
  },

  /** B1 进入课堂；成功返回 null，失败返回 8.3 文案 */
  async setNickname(nickname, pin) {
    try {
      await get().client.setNickname(nickname, pin)
      // 持久化「首次昵称已完成」到 gui.json（PRD 6.1 重启不再弹；失败不阻塞进入课堂）
      await invoke('set_nickname_done').catch(() => undefined)
      set({ nicknameDone: true, dialog: { ...get().dialog, nickname: false } })
      get().showToast('已进入课堂')
      await get().refresh()
      return null
    } catch (e) {
      return e instanceof Error ? e.message : '加入失败，请重试'
    }
  },

  setPasteBody(pasteBody) {
    set({ pasteBody })
  },
  showToast(toast) {
    set({ toast })
    window.setTimeout(() => {
      if (get().toast === toast) set({ toast: null })
    }, 2600)
  },

  applyNotify(n) {
    switch (n.t) {
      case 'ItemUpdated':
        set((s) => {
          const rest = s.items.filter((i) => i.id !== n.payload.item.id)
          return { items: [{ ...n.payload.item }, ...rest] }
        })
        // #16：学生提交（audience=Teacher 且 owner 为学员）→ 刷新分组树计数/最新时间
        if (
          get().role === 'teacher' &&
          n.payload.item.audience === 'Teacher' &&
          n.payload.item.owner_role === 'Student'
        ) {
          void get().loadSubmissions()
        }
        break
      case 'ShareReceived':
        // #16：同上（服务端对 upsert 会同时下发 ShareReceived 与 ItemUpdated）
        if (
          get().role === 'teacher' &&
          n.item.audience === 'Teacher' &&
          n.item.owner_role === 'Student'
        ) {
          void get().loadSubmissions()
        }
        break
      case 'ItemRemoved':
        set((s) => ({ items: s.items.filter((i) => i.id !== n.id) }))
        break
      case 'StatusChanged': {
        // 真实服务的通知文本不携带可解析语义（格式不受契约约束）；
        // 一律重拉结构化 status()（修订 #6④ 快照）判定连接态，避免文本误判。
        void get().refresh()
        break
      }
      case 'ShareReceived':
      case 'TransferProgress':
        // T12 接进度条；当前不消费
        break
      case 'Discovered': {
        // B1 自动发现房间列表（TC-01 步骤 4：须显示真实房间码，不得硬编码）
        const rooms = n.beacons.map((b) => ({
          room: b.room,
          host: b.host_name,
          pinRequired: b.pin_required,
        }))
        set({ discovered: rooms })
        break
      }
    }
  },
}))
