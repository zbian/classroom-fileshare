/**
 * EX-06 同步异常提示条（PRD 5.7 / 7.3 / 8.3；M2 #103 + #20b 接真实数据）。
 *
 * 数据来源（全部真实，不造状态）：
 * 1) `SvcNotify::Warning { kind, item_id, detail }`（#103 契约，经 retag → `{t:'Warning', warning}`）
 *    - RetryExhausted：占用退避重试耗尽，条目转 Failed（PRD EX-06「连续失败 5 次后提示手动处理」）
 *    - BackupFailed：旧版 `.bak` 写入失败（新版本已成功落盘，不回滚，PRD 5.7「记录告警」）
 *    - DiskFull：磁盘空间不足（预留）
 * 2) 列表条目的 `retry_count`（#103 契约，0..=5）与 `sync_state`：
 *    - RetryLocked：显示「第 n/5 次」
 *    - Failed：连续重试失败 → 提示手动处理
 *
 * 交互：告警可「忽略」（本地清除，仅清展示不改变条目状态）；「刷新」重拉列表。
 */
import { useMainStore } from '../store'

/** 重试上限（与 Rust 侧 #103 契约一致：≤5 次） */
const MAX_RETRY = 5

const KIND_TEXT: Record<string, string> = {
  RetryExhausted: '占用重试已用尽',
  BackupFailed: '旧版备份写入失败（新版本已保存）',
  DiskFull: '磁盘空间不足',
}

export default function SyncIssuesBanner() {
  const items = useMainStore((s) => s.items)
  const warnings = useMainStore((s) => s.warnings)
  const refresh = useMainStore((s) => s.refresh)
  const dismissWarnings = useMainStore((s) => s.dismissWarnings)

  const retrying = items.filter((i) => i.sync_state === 'RetryLocked')
  const failed = items.filter((i) => i.sync_state === 'Failed')
  const hasRealWarnings = warnings.length > 0
  if (retrying.length === 0 && failed.length === 0 && !hasRealWarnings) return null

  const parts: string[] = []
  if (retrying.length > 0) {
    const maxRound = Math.max(...retrying.map((i) => i.retry_count ?? 0))
    const suffix = maxRound > 0 ? `（第 ${maxRound}/${MAX_RETRY} 次）` : ''
    parts.push(`${retrying.length} 条目标文件正被程序占用，已加入重试队列${suffix}`)
  }
  if (failed.length > 0) parts.push(`${failed.length} 条连续重试失败`)
  const needManual = failed.length > 0 || warnings.some((w) => w.kind === 'RetryExhausted')
  const backupFailed = warnings.filter((w) => w.kind === 'BackupFailed')
  const otherWarnings = warnings.filter((w) => w.kind !== 'BackupFailed' && w.kind !== 'RetryExhausted')

  return (
    <div
      className={`info ${needManual ? 'err' : 'warn'}`}
      style={{ marginBottom: 10, display: 'flex', alignItems: 'flex-start', gap: 8 }}
      role="alert"
    >
      <div style={{ flex: 1 }}>
        <div>
          {parts.length > 0 ? parts.join('；') : '同步异常'}
          {needManual
            ? '　重试已用尽：请关闭占用该文件的程序后重新发送，或联系管理员处理（EX-06）。'
            : '　已加入重试队列，稍后自动覆盖；不影响其它共享。'}
        </div>

        {/* 真实 Warning 事件（#103）：逐条展示 detail，可忽略 */}
        {[...backupFailed, ...otherWarnings].slice(0, 4).map((w, idx) => (
          <div key={`${w.kind}-${w.item_id}-${idx}`} style={{ marginTop: 4, fontSize: 12 }}>
            · {KIND_TEXT[w.kind] ?? w.kind}
            {w.detail ? `：${w.detail}` : ''}
          </div>
        ))}
      </div>
      {hasRealWarnings ? (
        <button className="btn" style={{ padding: '2px 10px', fontSize: 12 }} onClick={dismissWarnings}>
          忽略
        </button>
      ) : null}
      <button className="btn" style={{ padding: '2px 10px', fontSize: 12 }} onClick={() => void refresh()}>
        刷新
      </button>
    </div>
  )
}
