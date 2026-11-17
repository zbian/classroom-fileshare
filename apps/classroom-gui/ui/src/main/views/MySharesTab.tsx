/**
 * C1「我共享的」/ D2「我共享的」六列列表（issue #90：搜索 + 类型筛选 + 虚拟滚动）。
 *
 * 布局：div-grid 行（替代 table，便于虚拟滚动；视觉与设计稿一致），
 * ≥ 虚拟列表组件统一承载，500 条只渲染可视区。
 * 视觉列：类型 / 名称或内容 / 大小 / 时间 / 状态 / 操作。
 */
import { useMainStore } from '../store'
import { filterItems, isFilterActive } from '../listFilter'
import VirtualList from '../components/VirtualList'
import { isItemLocked, LOCKED_HINT } from '../lockedState'
import SyncIssuesBanner from '../components/SyncIssuesBanner'
import { COLS_MY_SHARES as COLS, ROW_HEIGHT } from './columns'
import type { ShareItem, SyncState } from '../../shared/ipc/types'


/** 状态 chip 映射（PRD 7.6 状态字典：文字+色块，不单靠颜色）；教师端与学员端（D2）标签不同 */
function stateChip(
  s: SyncState,
  flavor: 'teacher' | 'student',
  retryCount = 0,
): { cls: string; label: string } {
  // #20b（#103 契约）：占用重试显示进度「第 n/5 次」；上限 5 次后服务端转 Failed
  const retryLabel = retryCount > 0 ? `（第 ${retryCount}/5 次）` : ''
  if (flavor === 'student') {
    switch (s) {
      case 'Synced':
        return { cls: 'ok', label: '已送达' }
      case 'PendingSend':
        return { cls: 'warn', label: '待发送' }
      case 'PendingSync':
        return { cls: 'warn', label: '待同步' }
      // PRD 7.4：教师已删除、本机副本待补删 → 用户可见文案「待删除」
      case 'DeleteDeferred':
        return { cls: 'warn', label: '待删除' }
      case 'RetryLocked':
        return { cls: 'err', label: '被占用重试' + retryLabel }
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
      return { cls: 'err', label: '被占用重试' + retryLabel }
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
  // M2 #19：教师侧行内重命名入口
  const openDialog = useMainStore((s) => s.openDialog)
  const st = stateChip(item.sync_state, flavor, item.retry_count ?? 0)
  const isFile = item.kind === 'File'
  // PRD 7.4：待删除条目（deleted 墓碑 / DeleteDeferred）不可打开、不可复制
  // （判定与提示文案收口在 shared 模块，工具栏入口复用同一判定）
  const locked = isItemLocked(item)
  return (
    <div
      className={`grow${selectedId === item.id ? ' selected' : ''}`}
      style={{ gridTemplateColumns: COLS, height: ROW_HEIGHT }}
      onClick={() => select(selectedId === item.id ? null : item.id)}
    >
      <span>{isFile ? <span className="badge file">文件</span> : <span className="badge text">文本</span>}</span>
      <span className="name" title={item.title}>
        {item.title}
      </span>
      <span>{isFile ? fmtSize(item.size) : `${item.size} 字`}</span>
      <span>{fmtTime(item.updated_at)}</span>
      <span>
        <span className={`chip ${st.cls}`}>{st.label}</span>
      </span>
      <span className="ops">
        {locked ? (
          // PRD 7.4「待删除」：打开/复制入口一律禁用（保留位置，样式置灰）
          <>
            <span className="disabled" title={LOCKED_HINT} aria-disabled="true">
              打开
            </span>
            <span className="disabled" title={LOCKED_HINT} aria-disabled="true">
              复制
            </span>
          </>
        ) : isFile ? (
          <>
            <span
              onClick={(e) => {
                e.stopPropagation()
                void client.openItem(item.id)
              }}
            >
              打开
            </span>
            {flavor === 'teacher' ? (
              <span
                onClick={(e) => {
                  e.stopPropagation()
                  openDialog('rename', item.id)
                }}
                title="重命名（广播给学员端，学员端 2 秒内同步）"
              >
                重命名
              </span>
            ) : (
              <span className="disabled" title="学员端重命名在 M2 未提供">
                更多
              </span>
            )}
          </>
        ) : (
          <>
            <span className="disabled" title="T12 接剪贴板">
              复制
            </span>
            {flavor === 'teacher' ? (
              <span
                onClick={(e) => {
                  e.stopPropagation()
                  openDialog('rename', item.id)
                }}
                title="重命名（广播给学员端，学员端 2 秒内同步）"
              >
                重命名
              </span>
            ) : (
              <span className="disabled" title="M2 重命名/编辑（学员端重命名未提供）">
                编辑
              </span>
            )}
          </>
        )}
      </span>
    </div>
  )
}

export default function MySharesTab({ flavor = 'teacher' }: { flavor?: 'teacher' | 'student' }) {
  const items = useMainStore((s) => s.items)
  const keyword = useMainStore((s) => s.keyword)
  const kindFilter = useMainStore((s) => s.kindFilter)

  const filter = { keyword, kind: kindFilter }
  const visible = filterItems(items, filter)
  const filtering = isFilterActive(filter)
  const pendingCount =
    flavor === 'student' ? items.filter((i) => i.sync_state === 'PendingSend').length : 0

  // 原始为空（无筛选）才算「没有任何内容」
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
        <span>操作</span>
      </div>
      {visible.length === 0 ? (
        <div style={{ padding: '36px 0' }}>
          <div className="info" style={{ textAlign: 'center' }}>
            无匹配结果：请调整搜索关键字或类型筛选（清空即可恢复全部）。
          </div>
        </div>
      ) : (
        <VirtualList
          items={visible}
          rowHeight={ROW_HEIGHT}
          renderRow={(item) => <Row key={item.id} item={item} flavor={flavor} />}
        />
      )}
    </>
  )
}
