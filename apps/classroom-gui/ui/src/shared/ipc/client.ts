/**
 * GUI ↔ 服务 ipc 客户端抽象（Task 9 UI 先行）。
 *
 * - GuiClient：UI 唯一依赖的接口（请求方法 + notify 订阅）。
 * - MockGuiClient：内存数据 + 主动推送模拟，dev 模式自动启用（hash 加 #realIpc 可切真实现），
 *   使全部 UI（列表/弹窗/状态条/EX-10）可独立开发验证。
 * - RealGuiClient：薄壳，走 Tauri invoke/listen 到 src-tauri/src/ipc_client.rs（T-7 合并后接线）。
 *
 * 事件负载形态与 crates/ipc 的 SvcNotify 对齐（见 types.ts）。
 */
import type { GuiReply, ListFilter, ServiceStatus, ShareItem, SvcNotify } from './types'

export type NotifyHandler = (n: SvcNotify) => void

export interface GuiClient {
  list(filter: ListFilter): Promise<ShareItem[]>
  sendText(title: string, body: string): Promise<GuiReply>
  sendFiles(paths: string[]): Promise<GuiReply>
  deleteItem(id: string): Promise<GuiReply>
  openItem(id: string): Promise<GuiReply>
  configureRoom(room: string, pin: string | null, allowDiscovery: boolean): Promise<GuiReply>
  status(): Promise<ServiceStatus>
  pauseTips(paused: boolean): Promise<GuiReply>
  /** 订阅服务通知；返回退订函数 */
  subscribe(h: NotifyHandler): () => void
  /** dev 辅助：模拟服务异常/恢复（EX-10 验证） */
  setServiceDown?(down: boolean): void
}

// ---------------------------------------------------------------------------
// Mock：内存仓库
// ---------------------------------------------------------------------------

const now = () => Date.now()

function seedItems(): ShareItem[] {
  const t = (title: string, size: number, mins: number): ShareItem => ({
    id: `seed-${title}`,
    kind: 'Text',
    owner_id: 'teacher',
    owner_role: 'Teacher',
    audience: 'All',
    title,
    size,
    created_at: now() - mins * 60_000,
    updated_at: now() - mins * 60_000,
    version: 1,
    deleted: false,
    sync_state: 'Synced',
  })
  const f = (title: string, size: number, mins: number, state: ShareItem['sync_state'] = 'Synced'): ShareItem => ({
    ...t(title, size, mins),
    id: `seed-${title}`,
    kind: 'File',
    sync_state: state,
  })
  return [
    f('第 3 章课件.pdf', 2_516_582, 88),
    t('请完成 P42 练习题', 128, 85),
    f('实验素材.zip', 19_505_152, 76),
    t('下课前提交实验结果', 42, 61),
    f('参考答案.pdf', 880_640, 37),
    t('补充：第 3 题不计分', 31, 12),
  ]
}

/** 解析 mock 的 StatusText（真实服务的文本形态接线时对齐；建议后端结构化） */
export function parseStatusText(text: string): ServiceStatus {
  // mock 约定：service=running|down;room=XK-204;online=28
  const running = /service=running/.test(text)
  const room = /room=([^;]+)/.exec(text)?.[1] ?? ''
  const online = Number(/online=(\d+)/.exec(text)?.[1] ?? 0)
  return { running, room, online, raw: text }
}

export class MockGuiClient implements GuiClient {
  private items: ShareItem[] = seedItems()
  private handlers = new Set<NotifyHandler>()
  private down = false
  private room = 'XK-204'
  private online = 28
  private seq = 1

  private emit(n: SvcNotify) {
    for (const h of this.handlers) h(n)
  }

  private statusText() {
    return `service=${this.down ? 'down' : 'running'};room=${this.room};online=${this.online}`
  }

  list(filter: ListFilter): Promise<ShareItem[]> {
    if (this.down) return Promise.reject(new Error('后台服务未运行'))
    let rows = this.items.filter((i) => !i.deleted)
    if (filter.kind) rows = rows.filter((i) => i.kind === filter.kind)
    if (filter.keyword) {
      const kw = filter.keyword.trim()
      if (kw) rows = rows.filter((i) => i.title.includes(kw))
    }
    return Promise.resolve(rows)
  }

  sendText(title: string, body: string): Promise<GuiReply> {
    if (this.down) return Promise.reject(new Error('后台服务未运行'))
    const prev = this.items.find((i) => !i.deleted && i.kind === 'Text' && i.title === title)
    const item: ShareItem = {
      id: prev?.id ?? `text-${this.seq++}`,
      kind: 'Text',
      owner_id: 'teacher',
      owner_role: 'Teacher',
      audience: 'All',
      title,
      size: body.length,
      created_at: prev?.created_at ?? now(),
      updated_at: now(),
      version: (prev?.version ?? 0) + 1,
      deleted: false,
      sync_state: 'Synced',
    }
    if (prev) this.items = this.items.filter((i) => i !== prev)
    this.items.push(item)
    this.emit({ t: 'ItemUpdated', payload: { item, overwritten: !!prev, old_version: prev?.version ?? null } })
    return Promise.resolve({ t: 'Ok' })
  }

  sendFiles(paths: string[]): Promise<GuiReply> {
    // 发送链路 T12 接入；mock 仅登记占位条目供 UI 流程验证
    if (this.down) return Promise.reject(new Error('后台服务未运行'))
    for (const p of paths) {
      const name = p.split(/[\\/]/).pop() ?? p
      const item: ShareItem = {
        id: `file-${this.seq++}`,
        kind: 'File',
        owner_id: 'teacher',
        owner_role: 'Teacher',
        audience: 'All',
        title: name,
        size: 1024,
        created_at: now(),
        updated_at: now(),
        version: 1,
        deleted: false,
        sync_state: 'PendingSync',
      }
      this.items.push(item)
      this.emit({ t: 'ItemUpdated', payload: { item, overwritten: false, old_version: null } })
    }
    return Promise.resolve({ t: 'Ok' })
  }

  deleteItem(id: string): Promise<GuiReply> {
    this.items = this.items.filter((i) => i.id !== id)
    this.emit({ t: 'ItemRemoved', id })
    return Promise.resolve({ t: 'Ok' })
  }

  openItem(_id: string): Promise<GuiReply> {
    return Promise.resolve({ t: 'Ok' })
  }

  configureRoom(room: string, _pin: string | null, _allowDiscovery: boolean): Promise<GuiReply> {
    this.room = room
    this.emit({ t: 'StatusChanged', text: this.statusText() })
    return Promise.resolve({ t: 'Ok' })
  }

  status(): Promise<ServiceStatus> {
    return Promise.resolve(parseStatusText(this.statusText()))
  }

  pauseTips(_paused: boolean): Promise<GuiReply> {
    return Promise.resolve({ t: 'Ok' })
  }

  subscribe(h: NotifyHandler): () => void {
    this.handlers.add(h)
    return () => this.handlers.delete(h)
  }

  setServiceDown(down: boolean) {
    this.down = down
    this.emit({ t: 'StatusChanged', text: this.statusText() })
  }
}

// ---------------------------------------------------------------------------
// Real：Tauri invoke/listen 薄壳（Rust 桥 src-tauri/src/ipc_client.rs）。
//   invoke('svc_request', { request: GuiRequest }) -> GuiReply
//   （GuiRequest/GuiReply 为 serde 外部不可辨名形态，snake_case 与 wire 一致）
//   listen('svc://notify', SvcNotify)                            （重打标签 {t,payload}）
// 服务未运行时 invoke reject（Error.message 为人类可读文案 → EX-10）。
// ---------------------------------------------------------------------------

export class RealGuiClient implements GuiClient {
  private handlers = new Set<NotifyHandler>()

  private async request(req: Record<string, unknown>): Promise<GuiReply> {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<GuiReply>('svc_request', { request: req })
  }

  list(filter: ListFilter): Promise<ShareItem[]> {
    return this.request({ List: filter }).then((r) => (r.t === 'Items' ? r.items : []))
  }
  sendText(title: string, body: string): Promise<GuiReply> {
    return this.request({ SendText: { title, body } })
  }
  sendFiles(paths: string[]): Promise<GuiReply> {
    return this.request({ SendFiles: { paths } })
  }
  deleteItem(id: string): Promise<GuiReply> {
    return this.request({ DeleteItem: { id } })
  }
  openItem(id: string): Promise<GuiReply> {
    return this.request({ OpenItem: { id } })
  }
  configureRoom(room: string, pin: string | null, allow_discovery: boolean): Promise<GuiReply> {
    return this.request({ ConfigureRoom: { room, pin, allow_discovery } })
  }
  async status(): Promise<ServiceStatus> {
    // 修订 #6④：结构化快照（service_state/room/online_count）
    const r = await this.request({ Status: null })
    if (r.t === 'Status') {
      return {
        running: r.snapshot.service_state === 'running',
        room: r.snapshot.room,
        online: r.snapshot.online_count,
        raw: JSON.stringify(r.snapshot),
      }
    }
    return { running: false, room: '', online: 0, raw: '' }
  }
  pauseTips(paused: boolean): Promise<GuiReply> {
    return this.request({ PauseTips: paused })
  }
  subscribe(h: NotifyHandler): () => void {
    this.handlers.add(h)
    let unlisten: (() => void) | null = null
    void import('@tauri-apps/api/event').then(async ({ listen }) => {
      unlisten = await listen<SvcNotify>('svc://notify', (e) => {
        for (const fn of this.handlers) fn(e.payload)
      })
    })
    return () => {
      this.handlers.delete(h)
      unlisten?.()
    }
  }
}

/** dev 模式默认 mock（#realIpc 强制真实现）；生产构建用真实现。 */
export function getClient(): GuiClient {
  const mock = import.meta.env.DEV && !window.location.hash.includes('realIpc')
  const client = mock ? new MockGuiClient() : new RealGuiClient()
  if (mock) {
    // dev 辅助入口：控制台可 window.__gui.setServiceDown(true) 验证 EX-10
    ;(window as unknown as { __gui?: GuiClient }).__gui = client
  }
  return client
}
