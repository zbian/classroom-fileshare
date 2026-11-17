/**
 * EX-06 同步异常提示条（PRD 5.7 / 7.3）。
 *
 * 服务端对「目标文件被其他程序占用」的写入进入退避重试（sync_state=RetryLocked，
 * 界面显示「被占用重试」，不阻塞其它共享）；连续失败后标记 Failed，
 * 此时按 PRD 提示教师手动处理。
 *
 * 说明：重试次数上限（≤5 次）与退避节奏由服务端/store 实现，GUI 只呈现状态；
 * 「备份写入失败告警」目前无 IPC 契约字段（ShareItem/SvcNotify 均无），
 * 已在 issue #20 的 PR 中说明需后端补字段（建议 Warning 通知或 ShareItem 增
 * warning 字段），本组件预留 Failed 聚合展示。
 */
import { useMainStore } from '../store'

export default function SyncIssuesBanner() {
  const items = useMainStore((s) => s.items)
  const refresh = useMainStore((s) => s.refresh)

  const retrying = items.filter((i) => i.sync_state === 'RetryLocked').length
  const failed = items.filter((i) => i.sync_state === 'Failed').length
  if (retrying === 0 && failed === 0) return null

  const parts: string[] = []
  if (retrying > 0) parts.push(`${retrying} 条被其他程序占用，正在退避重试`)
  if (failed > 0) parts.push(`${failed} 条连续重试失败`)
  const needManual = failed > 0

  return (
    <div
      className={`info ${needManual ? 'err' : 'warn'}`}
      style={{ marginBottom: 10, display: 'flex', alignItems: 'center', gap: 8 }}
      role="alert"
    >
      <span style={{ flex: 1 }}>
        {parts.join('；')}
        {needManual
          ? '　重试已用尽：请关闭占用该文件的程序后重新发送，或联系管理员处理（EX-06）。'
          : '　已加入重试队列，稍后自动覆盖；不影响其它共享。'}
      </span>
      <button
        className="btn"
        style={{ padding: '2px 10px', fontSize: 12 }}
        onClick={() => void refresh()}
      >
        刷新
      </button>
    </div>
  )
}
