/**
 * C1「我共享的」六列表格：类型/名称或内容/大小/时间/状态/操作。
 * 行选中 + 工具栏删除（E4）；文件行「打开」、文本行「复制/编辑」。
 */
import { useMainStore } from '../store'
import type { ShareItem, SyncState } from '../../shared/ipc/types'

/** 状态 chip 映射（PRD 7.6 状态字典：文字+色块，不单靠颜色）；教师端与学员端（D2）标签不同 */
function stateChip(s: SyncState, flavor: 'teacher' | 'student'): { cls: string; label: string } {
  if (flavor === 'student') {
    switch (s) {
      case 'Synced':
        return { cls: 'ok', label: '已送达' }
      case 'PendingSend':
        return { cls: 'warn', label: '待发送' }
      case 'PendingSync':
        return { cls: 'warn', label: '待同步' }
      case 'DeleteDeferred':
        return { cls: 'warn', label: '延迟删除' }
      case 'RetryLocked':
        return { cls: 'err', label: '被占用重试' }
      case 'Failed':
        return { cls: 'err', label: '失败' }
    }
  }
  switch (s) {
    case 'Synced':
      return { cls: 'ok', label: '已同步' }
    case 'PendingSend':
      return { cls: 'warn', label: '待发送' }
    case 'PendingSync':
      return { cls: 'warn', label: '待同步' }
    case 'DeleteDeferred':
      return { cls: 'warn', label: '延迟删除' }
    case 'RetryLocked':
      return { cls: 'err', label: '被占用重试' }
    case 'Failed':
      return { cls: 'err', label: '失败' }
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

function Row({ item, flavor }: { item: ShareItem; flavor: 'teacher' | 'student' }) {
  const selectedId = useMainStore((s) => s.selectedId)
  const select = useMainStore((s) => s.select)
  const client = useMainStore((s) => s.client)
  const st = stateChip(item.sync_state, flavor)
  const isFile = item.kind === 'File'
  return (
    <tr
      className={`row${selectedId === item.id ? ' selected' : ''}`}
      onClick={() => select(selectedId === item.id ? null : item.id)}
    >
      <td>{isFile ? <span className="badge file">文件</span> : <span className="badge text">文本</span>}</td>
      <td className="name" title={item.title}>
        {item.title}
      </td>
      <td>{isFile ? fmtSize(item.size) : `${item.size} 字`}</td>
      <td>{fmtTime(item.updated_at)}</td>
      <td>
        <span className={`chip ${st.cls}`}>{st.label}</span>
      </td>
      <td className="ops">
        {isFile ? (
          <>
            <span
              onClick={(e) => {
                e.stopPropagation()
                void client.openItem(item.id)
              }}
            >
              打开
            </span>
            <span className="disabled" title="T12 接入">
              更多
            </span>
          </>
        ) : (
          <>
            <span className="disabled" title="T12 接剪贴板">
              复制
            </span>
            <span className="disabled" title="M2 重命名/编辑">
              编辑
            </span>
          </>
        )}
      </td>
    </tr>
  )
}

export default function MySharesTab({ flavor = 'teacher' }: { flavor?: 'teacher' | 'student' }) {
  const items = useMainStore((s) => s.items)
  const pendingCount = flavor === 'student' ? items.filter((i) => i.sync_state === 'PendingSend').length : 0

  if (items.length === 0) {
    return (
      <div style={{ padding: '48px 0' }}>
        <div className="info" style={{ textAlign: 'center' }}>
          {flavor === 'student'
            ? '暂无共享内容：点击「+ 粘贴文本」，或把文件拖到桌面浮动图标。'
            : '暂无共享内容：点击工具栏「+ 粘贴文本」，或把文件拖到桌面浮动图标。'}
        </div>
      </div>
    )
  }

  return (
    <>
      {pendingCount > 0 ? (
        <div className="info warn" style={{ marginBottom: 10 }}>
          离线待发队列：{pendingCount} 条　·　重新连上教师机后按时间戳自动补发，不会丢失。
        </div>
      ) : null}
      <table className="tbl">
        <thead>
          <tr>
            <th style={{ width: 76 }}>类型</th>
            <th>名称 / 内容</th>
            <th style={{ width: 90 }}>大小</th>
            <th style={{ width: 70 }}>时间</th>
            <th style={{ width: 110 }}>状态</th>
            {flavor === 'teacher' ? <th style={{ width: 150 }}>操作</th> : <th style={{ width: 150 }}>操作</th>}
          </tr>
        </thead>
        <tbody>
          {items.map((it) => (
            <Row key={it.id} item={it} flavor={flavor} />
          ))}
        </tbody>
      </table>
    </>
  )
}
