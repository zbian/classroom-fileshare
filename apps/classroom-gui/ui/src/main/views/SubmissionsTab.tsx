/**
 * C2「学生提交」：左树右表（PRD 5.5 / 6.2 / 6.3；issue #16）。
 *
 * 数据面（#94 契约）：
 * - 左树：`client.submissions()` → SubmissionGroup{owner_id,count,latest_at}（latest_at 倒序）
 * - 右表：`client.list({ audience:'Teacher', owner: 昵称 })` 取该学员条目
 *
 * 只读约束（PRD 5.5）：学生提交条目**不可改名/删除/覆盖**，本视图不渲染任何
 * 写操作（无状态切换、无删除、无「更多」菜单）。
 * 未读：后端无未读字段 → 本地已读水位（store.seenOwners，按 owner 记 latest_at）。
 */
import { useMainStore } from '../store'
import VirtualList from '../components/VirtualList'
import { COLS_TEACHER_SHARES as COLS, ROW_HEIGHT } from './columns'
import type { ShareItem } from '../../shared/ipc/types'

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

/** 最新时间：同日显示 HH:MM，跨日显示 MM-DD（左树紧凑展示） */
function fmtLatest(ts: number): string {
  const d = new Date(ts)
  const now = new Date()
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate()
  const hh = String(d.getHours()).padStart(2, '0')
  const mm = String(d.getMinutes()).padStart(2, '0')
  return sameDay ? `${hh}:${mm}` : `${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')} ${hh}:${mm}`
}

function SubRow({ item }: { item: ShareItem }) {
  const selectedId = useMainStore((s) => s.selectedId)
  const select = useMainStore((s) => s.select)
  return (
    <div
      className={`grow${selectedId === item.id ? ' selected' : ''}`}
      style={{ gridTemplateColumns: COLS, height: ROW_HEIGHT }}
      onClick={() => select(selectedId === item.id ? null : item.id)}
      title="学生提交（只读）：不可改名 / 删除 / 覆盖"
    >
      <span>
        {item.kind === 'File' ? <span className="badge file">文件</span> : <span className="badge text">文本</span>}
      </span>
      <span className="name" title={item.title}>
        {item.title}
      </span>
      <span>{item.kind === 'File' ? fmtSize(item.size) : `${item.size} 字`}</span>
      <span>{fmtLatest(item.updated_at)}</span>
      <span>
        <span className="chip ok">已提交</span>
      </span>
    </div>
  )
}

export default function SubmissionsTab() {
  const groups = useMainStore((s) => s.groups)
  const selectedOwner = useMainStore((s) => s.selectedOwner)
  const selectOwner = useMainStore((s) => s.selectOwner)
  const items = useMainStore((s) => s.submissionItems)
  const seenOwners = useMainStore((s) => s.seenOwners)
  const serviceDown = useMainStore((s) => s.serviceDown)
  const retryService = useMainStore((s) => s.retryService)

  if (serviceDown) {
    return (
      <div style={{ padding: '48px 0' }}>
        <div className="info" style={{ textAlign: 'center' }}>
          后台服务未运行，无法加载学生提交。<button className="btn" onClick={() => void retryService()}>重试</button>
        </div>
      </div>
    )
  }

  if (groups.length === 0) {
    return (
      <div style={{ padding: '48px 0' }}>
        <div className="info" style={{ textAlign: 'center' }}>
          暂无学生提交：学员端「发送给教师」后，将按昵称分组出现在这里。
        </div>
      </div>
    )
  }

  const current = groups.find((g) => g.owner_id === selectedOwner)

  return (
    <div className="sub-layout">
      {/* 左树：按学员昵称分组（计数 / 最新时间 / 未读高亮） */}
      <aside className="sub-tree" aria-label="学生提交分组">
        <div className="sub-tree-head">学生（{groups.length}）</div>
        {groups.map((g) => {
          const unread = g.latest_at > (seenOwners[g.owner_id] ?? 0)
          return (
            <button
              key={g.owner_id}
              className={`sub-node${g.owner_id === selectedOwner ? ' active' : ''}${unread ? ' unread' : ''}`}
              onClick={() => void selectOwner(g.owner_id)}
              title={`${g.owner_id}：${g.count} 条提交（最新 ${fmtLatest(g.latest_at)}）`}
            >
              <span className="sub-avatar" aria-hidden="true">
                {g.owner_id.slice(0, 1)}
              </span>
              <span className="sub-name">{g.owner_id}</span>
              <span className="sub-count">{g.count}</span>
              <span className="sub-time">{fmtLatest(g.latest_at)}</span>
              {unread ? <span className="sub-dot" title="有新的提交" /> : null}
            </button>
          )
        })}
      </aside>

      {/* 右表：选中学员的提交（只读） */}
      <section className="sub-main">
        {!current ? (
          <div className="info" style={{ padding: '24px 0' }}>
            从左侧选择学员查看其提交。
          </div>
        ) : items.length === 0 ? (
          <div className="info" style={{ padding: '24px 0' }}>
            {current.owner_id} 暂无提交内容。
          </div>
        ) : (
          <>
            <div className="list-meta">
              {current.owner_id} · 共 {current.count} 条提交（只读）
            </div>
            <div className="grid-head" style={{ gridTemplateColumns: COLS }}>
              <span>类型</span>
              <span>名称 / 内容</span>
              <span>大小</span>
              <span>时间</span>
              <span>状态</span>
            </div>
            <VirtualList
              items={items}
              rowHeight={ROW_HEIGHT}
              renderRow={(item) => <SubRow key={item.id} item={item} />}
            />
          </>
        )}
      </section>
    </div>
  )
}
