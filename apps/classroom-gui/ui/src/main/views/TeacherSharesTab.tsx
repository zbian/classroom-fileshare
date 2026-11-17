/**
 * D1 教师共享 · 只读列表（FR-S-04；issue #90：搜索 + 类型筛选 + 虚拟滚动）。
 * **不出现任何增删改控件**（review 硬性条目）——工具栏只提供搜索/筛选与只读操作。
 */
import { useMainStore } from '../store'
import { filterItems, isFilterActive } from '../listFilter'
import VirtualList from '../components/VirtualList'
import SyncIssuesBanner from '../components/SyncIssuesBanner'
import { COLS_TEACHER_SHARES as COLS, ROW_HEIGHT } from './columns'
import type { ShareItem, SyncState } from '../../shared/ipc/types'


/** D1 状态映射（PRD 6.4）：新/已下载/下载中（M1：下载中静态样例，进度 T12） */
function stateChip(s: SyncState, isNew: boolean): { cls: string; label: string } {
  if (isNew) return { cls: 'pri', label: '新' }
  switch (s) {
    case 'Synced':
      return { cls: 'ok', label: '已下载' }
    // PRD 7.4：教师已删除、本机待补删 → 用户可见文案「待删除」
    case 'DeleteDeferred':
      return { cls: 'warn', label: '待删除' }
    case 'PendingSync':
      return { cls: 'warn', label: '下载中' }
    default:
      return { cls: 'warn', label: s }
  }
}

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

function fmtTime(ts: number): string {
  const d = new Date(ts)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

function Row({ item }: { item: ShareItem & { is_new?: boolean } }) {
  const selectedId = useMainStore((s) => s.selectedId)
  const select = useMainStore((s) => s.select)
  const seen = useMainStore((s) => s.seenIds.includes(item.id))
  const isNew = !!item.is_new && !seen
  const st = stateChip(item.sync_state, isNew)
  return (
    <div
      className={`grow${selectedId === item.id ? ' selected' : ''}`}
      style={{ gridTemplateColumns: COLS, height: ROW_HEIGHT }}
      onClick={() => select(selectedId === item.id ? null : item.id)}
    >
      <span>
        {item.kind === 'File' ? <span className="badge file">文件</span> : <span className="badge text">文本</span>}
      </span>
      <span className="name" title={item.title}>
        {item.title}
      </span>
      <span>{item.kind === 'File' ? fmtSize(item.size) : `${item.size} 字`}</span>
      <span>{fmtTime(item.updated_at)}</span>
      <span>
        <span className={`chip ${st.cls}`}>{st.label}</span>
      </span>
    </div>
  )
}

export default function TeacherSharesTab() {
  const items = useMainStore((s) => s.items)
  const keyword = useMainStore((s) => s.keyword)
  const kindFilter = useMainStore((s) => s.kindFilter)

  const filter = { keyword, kind: kindFilter }
  const visible = filterItems(items, filter)
  const filtering = isFilterActive(filter)

  if (items.length === 0) {
    return (
      <div style={{ padding: '48px 0' }}>
        <div className="info" style={{ textAlign: 'center' }}>
          暂无教师共享内容；教师广播后将自动出现在这里。
        </div>
      </div>
    )
  }

  return (
    <>
      <div className="info" style={{ marginBottom: 10 }}>
        教师删除某条共享后，本机副本将同步移除；若当前离线，将在下次连上教师机时自动删除。
      </div>
      <SyncIssuesBanner />
      <div className="list-meta">
        {filtering ? `筛选后 ${visible.length} / ${items.length} 条` : `共 ${items.length} 条`}
      </div>
      <div className="grid-head" style={{ gridTemplateColumns: COLS }}>
        <span>类型</span>
        <span>名称 / 内容</span>
        <span>大小</span>
        <span>时间</span>
        <span>状态</span>
      </div>
      {visible.length === 0 ? (
        <div style={{ padding: '36px 0' }}>
          <div className="info" style={{ textAlign: 'center' }}>
            无匹配结果：请调整搜索关键字或类型筛选（清空即可恢复全部）。
          </div>
        </div>
      ) : (
        <VirtualList items={visible} rowHeight={ROW_HEIGHT} renderRow={(item) => <Row key={item.id} item={item} />} />
      )}
    </>
  )
}
