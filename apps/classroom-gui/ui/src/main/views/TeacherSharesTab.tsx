/**
 * D1 教师共享 · 只读列表（FR-S-04）：搜索、只读标识、「新」标记、
 * 复制选中文本 / 打开选中文件（经工具栏），**不出现任何增删改控件**（review 硬性条目）。
 */
import { useMainStore } from '../store'
import type { ShareItem, SyncState } from '../../shared/ipc/types'

/** D1 状态映射（PRD 6.4）：新/已下载/下载中（M1：下载中静态样例，进度 T12） */
function stateChip(s: SyncState, isNew: boolean): { cls: string; label: string } {
  if (isNew) return { cls: 'pri', label: '新' }
  switch (s) {
    case 'Synced':
      return { cls: 'ok', label: '已下载' }
    case 'PendingSync':
      return { cls: 'warn', label: '下载中' }
    case 'PendingSend':
      return { cls: 'warn', label: '待发送' }
    case 'DeleteDeferred':
      return { cls: 'warn', label: '延迟删除' }
    case 'RetryLocked':
      return { cls: 'err', label: '被占用重试' }
    case 'Failed':
      return { cls: 'err', label: '失败' }
    default:
      // 未知枚举不裸露英文（PRD 4.6 状态表达：文字标签 + 色块）
      return { cls: 'warn', label: '同步中' }
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
    <tr
      className={`row${selectedId === item.id ? ' selected' : ''}`}
      onClick={() => select(selectedId === item.id ? null : item.id)}
    >
      <td>{item.kind === 'File' ? <span className="badge file">文件</span> : <span className="badge text">文本</span>}</td>
      <td className="name" title={item.title}>
        {item.title}
      </td>
      <td>{item.kind === 'File' ? fmtSize(item.size) : `${item.size} 字`}</td>
      <td>{fmtTime(item.updated_at)}</td>
      <td>
        <span className={`chip ${st.cls}`}>{st.label}</span>
      </td>
    </tr>
  )
}

export default function TeacherSharesTab() {
  const items = useMainStore((s) => s.items)

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
      <table className="tbl">
        <thead>
          <tr>
            <th style={{ width: 76 }}>类型</th>
            <th>名称 / 内容</th>
            <th style={{ width: 90 }}>大小</th>
            <th style={{ width: 70 }}>时间</th>
            <th style={{ width: 110 }}>状态</th>
          </tr>
        </thead>
        <tbody>
          {items.map((it) => (
            <Row key={it.id} item={it} />
          ))}
        </tbody>
      </table>
    </>
  )
}
