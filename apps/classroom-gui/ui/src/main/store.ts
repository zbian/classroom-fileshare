/**
 * 主界面状态（zustand）。T9 教师视图 + T10 学员视图/首次昵称（mock 驱动）。
 * ipc notify → applyNotify；请求失败统一落入 serviceDown（教师 EX-10 / 学员 EX-08）。
 */
import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { getClient, type GuiClient, type LocalRole, type UiShareItem } from '../shared/ipc/client'
import type { ListFilter, ServiceStatus, SvcNotify } from '../shared/ipc/types'

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
  keyword: string
  kindFilter: 'all' | 'Text' | 'File'
  /** 已查看条目（D1「新」标记消除，客户端态） */
  seenIds: string[]
  dialog: DialogState
  toast: string | null
  // 动作
  init(): Promise<void>
  refresh(): Promise<void>
  retryService(): Promise<void>
  setActiveTab(t: TabKey): void
  select(id: string | null): void
  markSeen(id: string): void
  setKeyword(kw: string): void
  setKindFilter(k: 'all' | 'Text' | 'File'): void
  openDialog<K extends keyof DialogState>(key: K, id?: string): void
  closeDialog(key: keyof DialogState): void
  sendText(title: string, body: string): Promise<boolean>
  confirmDelete(): Promise<void>
  configureRoom(room: string, pin: string | null, allowDiscovery: boolean): Promise<boolean>
  setNickname(nickname: string, pin?: string | null): Promise<string | null>
  showToast(msg: string): void
  applyNotify(n: SvcNotify): void
}

/** 学员当前 Tab 对应的列表过滤 */
function studentFilter(tab: TabKey, keyword: string, kind: 'all' | 'Text' | 'File'): ListFilter {
  return {
    owner: tab === 'teacher' ? 'teacher' : null,
    keyword: keyword || null,
    kind: kind === 'all' ? null : kind,
  }
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
  seenIds: [],
  dialog: { pasteText: false, createRoom: false, nickname: false, deleteConfirm: null },
  toast: null,

  async init() {
    const { client } = get()
    client.subscribe((n) => get().applyNotify(n))
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
    await get().refresh()
  },

  async refresh() {
    const { client, keyword, kindFilter, role, activeTab } = get()
    const filter: ListFilter =
      role === 'student'
        ? { ...studentFilter(activeTab, keyword, kindFilter), include_deleted: false }
        : {
            keyword: keyword || null,
            kind: kindFilter === 'all' ? null : kindFilter,
            include_deleted: false, // wire 契约：bool 无 serde default，必须显式携带
          }
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
    set({ activeTab, selectedId: null })
    void get().refresh()
  },
  select(selectedId) {
    set({ selectedId })
    if (selectedId) get().markSeen(selectedId)
  },
  markSeen(id) {
    if (!get().seenIds.includes(id)) set({ seenIds: [...get().seenIds, id] })
  },
  setKeyword(keyword) {
    set({ keyword })
    void get().refresh()
  },
  setKindFilter(kindFilter) {
    set({ kindFilter })
    void get().refresh()
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
      get().showToast('已删除，正在同步到教师端')
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
