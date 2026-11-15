/**
 * 教师端主界面（main.html，Task 9）：AppShell 六层结构（PRD 8.2）。
 * 数据经 ipc 客户端抽象（dev 自动 mock，T-7 合并后接真实现）。
 * T10 将按 role 切换学员视图（D1/D2）。
 */
import { useEffect } from 'react'
import './main.css'
import { useMainStore, type TabKey } from './store'
import MySharesTab from './views/MySharesTab'
import PasteTextDialog from './dialogs/PasteTextDialog'
import CreateRoomDialog from './dialogs/CreateRoomDialog'
import DeleteConfirmDialog from './dialogs/DeleteConfirmDialog'

function TitleBar() {
  const status = useMainStore((s) => s.status)
  const online = status?.running ? status.online : 0
  return (
    <header className="app-titlebar">
      <span className="app-name">课堂共享</span>
      <span className="chip pri">教师端</span>
      <span style={{ flex: 1 }} />
      <span style={{ fontSize: 12, color: 'var(--sub)' }}>已连接 {online} 名学员</span>
    </header>
  )
}

function StatusBar() {
  const status = useMainStore((s) => s.status)
  const down = useMainStore((s) => s.serviceDown)
  const retry = useMainStore((s) => s.retryService)
  if (down) {
    // EX-10：服务未运行提示与重试
    return (
      <div className="app-statusbar warn" role="alert">
        <span className="status-dot warn" />
        <span>后台服务未运行，点击重启服务</span>
        <span style={{ flex: 1 }} />
        <button className="btn" onClick={() => void retry()}>
          重试
        </button>
      </div>
    )
  }
  return (
    <div className="app-statusbar">
      <span className="status-dot ok" />
      <span>后台服务 运行中</span>
      <span>·</span>
      <span>
        房间 <b>{status?.room || '—'}</b>
      </span>
      <span>·</span>
      <span>
        在线学员 <b>{status?.online ?? 0}</b>
      </span>
      <span>·</span>
      <span>开机自启 已开启</span>
      <span style={{ flex: 1 }} />
      <button className="btn" style={{ padding: '2px 10px', fontSize: 12 }} onClick={() => void retry()}>
        刷新
      </button>
    </div>
  )
}

function TabBar() {
  const activeTab = useMainStore((s) => s.activeTab)
  const setActiveTab = useMainStore((s) => s.setActiveTab)
  const items = useMainStore((s) => s.items)
  const tabs: { key: TabKey; label: string; disabled?: boolean; hint?: string }[] = [
    { key: 'mine', label: `我共享的 (${items.length})` },
    { key: 'submissions', label: '学生提交', disabled: true, hint: 'M2（FR-T-05）' },
    { key: 'records', label: '收发记录', disabled: true, hint: 'M2' },
  ]
  return (
    <nav className="app-tabs">
      {tabs.map((t) => (
        <button
          key={t.key}
          className={`app-tab${activeTab === t.key ? ' on' : ''}${t.disabled ? ' disabled' : ''}`}
          title={t.hint}
          onClick={() => !t.disabled && setActiveTab(t.key)}
        >
          {t.label}
        </button>
      ))}
    </nav>
  )
}

function Toolbar() {
  const keyword = useMainStore((s) => s.keyword)
  const setKeyword = useMainStore((s) => s.setKeyword)
  const kindFilter = useMainStore((s) => s.kindFilter)
  const setKindFilter = useMainStore((s) => s.setKindFilter)
  const openDialog = useMainStore((s) => s.openDialog)
  const selectedId = useMainStore((s) => s.selectedId)
  return (
    <div className="app-toolbar">
      <input
        className="input"
        placeholder="搜索名称 / 内容"
        value={keyword}
        onChange={(e) => setKeyword(e.target.value)}
      />
      <select className="select" value={kindFilter} onChange={(e) => setKindFilter(e.target.value as 'all' | 'Text' | 'File')}>
        <option value="all">全部类型</option>
        <option value="File">文件</option>
        <option value="Text">文本</option>
      </select>
      <span className="spacer" />
      <button className="btn" disabled title="T12 接文件选择器">
        + 添加文件
      </button>
      <button className="btn pri" onClick={() => openDialog('pasteText')}>
        + 粘贴文本
      </button>
      <button className="btn" disabled title="M2（rename 事件）">
        重命名
      </button>
      <button
        className="btn dg"
        disabled={!selectedId}
        title={selectedId ? '删除选中条目' : '先在列表选中一条'}
        onClick={() => selectedId && openDialog('deleteConfirm', selectedId)}
      >
        删除
      </button>
    </div>
  )
}

function FooterHint() {
  const openCreateRoom = useMainStore((s) => s.openDialog)
  return (
    <footer className="app-footer">
      <div className="info" style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ flex: 1 }}>
          把文件拖到此处 / 拖到桌面浮动图标 · 文本可直接 Ctrl+V 粘贴后共享
        </span>
        <button className="btn" style={{ padding: '2px 10px', fontSize: 12 }} onClick={() => openCreateRoom('createRoom')}>
          本节课设置
        </button>
      </div>
    </footer>
  )
}

function Toast() {
  const toast = useMainStore((s) => s.toast)
  if (!toast) return null
  return (
    <div
      style={{
        position: 'fixed',
        top: 56,
        left: '50%',
        transform: 'translateX(-50%)',
        zIndex: 200,
      }}
    >
      <div className="chip ok" style={{ padding: '6px 14px', fontSize: 13, background: '#fff', border: '1px solid var(--green)', boxShadow: '0 4px 12px rgba(15,23,42,.12)' }}>
        {toast}
      </div>
    </div>
  )
}

export default function App() {
  const init = useMainStore((s) => s.init)
  const activeTab = useMainStore((s) => s.activeTab)

  useEffect(() => {
    void init()
  }, [init])

  return (
    <div className="app">
      <TitleBar />
      <StatusBar />
      <TabBar />
      <Toolbar />
      <main className="app-content">{activeTab === 'mine' ? <MySharesTab /> : null}</main>
      <FooterHint />
      <Toast />
      <PasteTextDialog />
      <CreateRoomDialog />
      <DeleteConfirmDialog />
    </div>
  )
}
