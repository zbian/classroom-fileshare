/**
 * 主界面（main.html）：AppShell 六层结构（PRD 8.2）。
 * T9 教师视图（C1）；T10 学员视图（B1 首次昵称 + D1 教师共享只读 + D2 我共享的）。
 * 角色来自 gui.json（Rust get_icon_state；M1 由 --role= 写入）；数据经 ipc 抽象（dev mock）。
 */
import { useEffect } from 'react'
import './main.css'
import { useMainStore, type TabKey } from './store'
import MySharesTab from './views/MySharesTab'
import TeacherSharesTab from './views/TeacherSharesTab'
import PasteTextDialog from './dialogs/PasteTextDialog'
import CreateRoomDialog from './dialogs/CreateRoomDialog'
import DeleteConfirmDialog from './dialogs/DeleteConfirmDialog'
import NicknameDialog from './dialogs/NicknameDialog'

function TitleBar() {
  const role = useMainStore((s) => s.role)
  const status = useMainStore((s) => s.status)
  const online = status?.running ? status.online : 0
  return (
    <header className="app-titlebar">
      <span className="app-name">课堂共享</span>
      <span className="chip pri">{role === 'student' ? '学员端' : '教师端'}</span>
      <span style={{ flex: 1 }} />
      <span style={{ fontSize: 12, color: 'var(--sub)' }}>
        {role === 'student'
          ? status?.running
            ? `已连接 ${status.room}`
            : '未连接教师机'
          : `已连接 ${online} 名学员`}
      </span>
    </header>
  )
}

function StatusBar() {
  const role = useMainStore((s) => s.role)
  const status = useMainStore((s) => s.status)
  const down = useMainStore((s) => s.serviceDown)
  const retry = useMainStore((s) => s.retryService)
  if (down) {
    // 教师 EX-10 / 学员 EX-08（PRD 10）：文案按角色区分
    return (
      <div className="app-statusbar warn" role="alert">
        <span className="status-dot warn" />
        <span>
          {role === 'student'
            ? '未连接教师机，内容将在恢复后自动发送'
            : '后台服务未运行，点击重启服务'}
        </span>
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
      {role === 'student' ? (
        <>
          <span>已连接 教师机 {status?.room || '—'}</span>
          <span>·</span>
          <span>本地缓存 已同步</span>
        </>
      ) : (
        <>
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
        </>
      )}
      <span style={{ flex: 1 }} />
      <button className="btn" style={{ padding: '2px 10px', fontSize: 12 }} onClick={() => void retry()}>
        刷新
      </button>
    </div>
  )
}

function TabBar() {
  const role = useMainStore((s) => s.role)
  const activeTab = useMainStore((s) => s.activeTab)
  const setActiveTab = useMainStore((s) => s.setActiveTab)
  const items = useMainStore((s) => s.items)
  const teacherTabs: { key: TabKey; label: string; disabled?: boolean; hint?: string }[] = [
    { key: 'mine', label: `我共享的 (${items.length})` },
    { key: 'submissions', label: '学生提交', disabled: true, hint: 'M2（FR-T-05）' },
    { key: 'records', label: '收发记录', disabled: true, hint: 'M2' },
  ]
  const studentTabs: { key: TabKey; label: string; disabled?: boolean; hint?: string }[] = [
    { key: 'teacher', label: `教师共享 (${items.length})` },
    { key: 'mine', label: '我共享的' },
  ]
  const tabs = role === 'student' ? studentTabs : teacherTabs
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
  const role = useMainStore((s) => s.role)
  const activeTab = useMainStore((s) => s.activeTab)
  const keyword = useMainStore((s) => s.keyword)
  const setKeyword = useMainStore((s) => s.setKeyword)
  const kindFilter = useMainStore((s) => s.kindFilter)
  const setKindFilter = useMainStore((s) => s.setKindFilter)
  const openDialog = useMainStore((s) => s.openDialog)
  const selectedId = useMainStore((s) => s.selectedId)
  const selected = useMainStore((s) => s.items.find((i) => i.id === s.selectedId))
  const client = useMainStore((s) => s.client)
  const showToast = useMainStore((s) => s.showToast)

  const search = (
    <span className="search-wrap">
      <input
        className="input"
        placeholder={role === 'student' && activeTab === 'teacher' ? '搜索教师共享内容' : '搜索名称 / 内容'}
        value={keyword}
        onChange={(e) => setKeyword(e.target.value)}
      />
      {keyword ? (
        <button className="search-clear" title="清空搜索" onClick={() => setKeyword('')}>
          ×
        </button>
      ) : null}
    </span>
  )
  const kindSelect = (
    <select
      className="select"
      title="按类型筛选"
      value={kindFilter}
      onChange={(e) => setKindFilter(e.target.value as 'all' | 'Text' | 'File')}
    >
      <option value="all">全部类型</option>
      <option value="File">文件</option>
      <option value="Text">文本</option>
    </select>
  )

  // 学员 D1：只读工具栏（无任何增删改控件，review 硬性条目）
  if (role === 'student' && activeTab === 'teacher') {
    return (
      <div className="app-toolbar">
        {search}
        {kindSelect}
        <span className="chip">只读</span>
        <span className="spacer" />
        <button
          className="btn"
          disabled={selected?.kind !== 'Text'}
          title={selected?.kind === 'Text' ? '复制选中文本全文' : '先选中一条文本'}
          onClick={() => showToast('已复制到剪贴板（T12 接剪贴板写入）')}
        >
          复制选中文本
        </button>
        <button
          className="btn"
          disabled={selected?.kind !== 'File'}
          title={selected?.kind === 'File' ? '用系统默认程序打开' : '先选中一个文件'}
          onClick={() => selected && void client.openItem(selected.id)}
        >
          打开选中文件
        </button>
      </div>
    )
  }

  // 教师 C1 / 学员 D2：增删改工具栏（学员固定「发送对象：仅教师」）
  return (
    <div className="app-toolbar">
      {search}
      {kindSelect}
      <span className="spacer" />
      {role === 'student' ? <span className="chip warn">发送对象：仅教师</span> : null}
      <button
        className="btn"
        disabled
        title={role === 'student' ? 'M2（FR-S-03 学员文件回传）' : 'T12 接文件选择器'}
      >
        + 添加文件
      </button>
      <button
        className="btn pri"
        disabled={role === 'student'}
        title={role === 'student' ? 'M2（FR-S-02 学员文本回传）' : '粘贴剪贴板文本并共享'}
        onClick={() => openDialog('pasteText')}
      >
        + 粘贴文本
      </button>
      <button className="btn" disabled title="M2（rename 事件）">
        重命名
      </button>
      <button
        className="btn dg"
        disabled={!selectedId || role === 'student'}
        title={
          role === 'student'
            ? 'M2（FR-S-05 学员删除同步）'
            : selectedId
              ? '删除选中条目'
              : '先在列表选中一条'
        }
        onClick={() => selectedId && openDialog('deleteConfirm', selectedId)}
      >
        删除
      </button>
    </div>
  )
}

function FooterHint() {
  const role = useMainStore((s) => s.role)
  const openCreateRoom = useMainStore((s) => s.openDialog)
  return (
    <footer className="app-footer">
      <div className="info" style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ flex: 1 }}>
          {role === 'student'
            ? '拖入文件 / 文本，或 Ctrl+V 粘贴一段文字直接共享给教师'
            : '把文件拖到桌面浮动图标即可共享 · 文本可直接 Ctrl+V 粘贴后共享'}
        </span>
        {role === 'teacher' ? (
          <button
            className="btn"
            style={{ padding: '2px 10px', fontSize: 12 }}
            onClick={() => openCreateRoom('createRoom')}
          >
            本节课设置
          </button>
        ) : null}
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
      <div
        className="chip ok"
        style={{
          padding: '6px 14px',
          fontSize: 13,
          background: '#fff',
          border: '1px solid var(--green)',
          boxShadow: '0 4px 12px rgba(15,23,42,.12)',
        }}
      >
        {toast}
      </div>
    </div>
  )
}

export default function App() {
  const init = useMainStore((s) => s.init)
  const role = useMainStore((s) => s.role)
  const activeTab = useMainStore((s) => s.activeTab)

  useEffect(() => {
    void init()
  }, [init])

  // 主题切换（tokens.css：教师蓝 / 学员青）
  useEffect(() => {
    document.documentElement.dataset.role = role === 'student' ? 'student' : 'teacher'
  }, [role])

  const isStudentTeacherTab = role === 'student' && activeTab === 'teacher'

  return (
    <div className="app">
      <TitleBar />
      <StatusBar />
      <TabBar />
      <Toolbar />
      <main className="app-content">
        {isStudentTeacherTab ? (
          <TeacherSharesTab />
        ) : (
          <MySharesTab flavor={role === 'student' ? 'student' : 'teacher'} />
        )}
      </main>
      <FooterHint />
      <Toast />
      <PasteTextDialog />
      {role !== 'student' ? <CreateRoomDialog /> : null}
      <DeleteConfirmDialog />
      <NicknameDialog />
    </div>
  )
}
