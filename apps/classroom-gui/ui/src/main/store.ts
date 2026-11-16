/**
 * 教师端主界面状态（zustand，Task 9）。
 * ipc notify → applyNotify；请求失败统一落入 serviceDown（EX-10）。
 */
import { create } from 'zustand'
import { getClient, type GuiClient } from '../shared/ipc/client'
import type { ListFilter, ServiceStatus, ShareItem, SvcNotify } from '../shared/ipc/types'

export type TabKey = 'mine' | 'submissions' | 'records'

export interface DialogState {
  pasteText: boolean
  createRoom: boolean
  deleteConfirm: string | null // 待删除条目 id
}

interface MainStore {
  client: GuiClient
  items: ShareItem[]
  status: ServiceStatus | null
  serviceDown: boolean
  activeTab: TabKey
  selectedId: string | null
  keyword: string
  kindFilter: 'all' | 'Text' | 'File'
  dialog: DialogState
  toast: string | null
  // 动作
  init(): Promise<void>
  refresh(): Promise<void>
  retryService(): Promise<void>
  setActiveTab(t: TabKey): void
  select(id: string | null): void
  setKeyword(kw: string): void
  setKindFilter(k: 'all' | 'Text' | 'File'): void
  openDialog<K extends keyof DialogState>(key: K, id?: string): void
  closeDialog(key: keyof DialogState): void
  sendText(title: string, body: string): Promise<boolean>
  confirmDelete(): Promise<void>
  configureRoom(room: string, pin: string | null, allowDiscovery: boolean): Promise<boolean>
  showToast(msg: string): void
  applyNotify(n: SvcNotify): void
}

export const useMainStore = create<MainStore>((set, get) => ({
  client: getClient(),
  items: [],
  status: null,
  serviceDown: false,
  activeTab: 'mine',
  selectedId: null,
  keyword: '',
  kindFilter: 'all',
  dialog: { pasteText: false, createRoom: false, deleteConfirm: null },
  toast: null,

  async init() {
    const { client } = get()
    client.subscribe((n) => get().applyNotify(n))
    await get().refresh()
  },

  async refresh() {
    const { client, keyword, kindFilter } = get()
    const filter: ListFilter = {
      keyword: keyword || null,
      kind: kindFilter === 'all' ? null : kindFilter,
      include_deleted: false, // wire 契约：bool 无 serde default，必须显式携带
    }
    try {
      const [items, status] = await Promise.all([client.list(filter), client.status()])
      set({ items, status, serviceDown: !status.running })
    } catch {
      // EX-10：服务未运行/未接线 —— 列表置空并提示重试
      set({ serviceDown: true, items: [] })
    }
  },

  async retryService() {
    await get().refresh()
  },

  setActiveTab(activeTab) {
    set({ activeTab })
  },
  select(selectedId) {
    set({ selectedId })
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
      get().showToast('已共享给全体学员')
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
      get().showToast('已删除，正在同步到学员端')
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
          return { items: [n.payload.item, ...rest] }
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
      case 'Discovered':
        // T9 教师端暂不消费（学员端视图/进度条/发现列表在 T10/T12）
        break
    }
  },
}))
