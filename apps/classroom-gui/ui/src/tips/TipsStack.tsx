/**
 * tips.html 占位（Task 1b）：提醒气泡堆叠（A2）。
 * T11 实现：15s 倒计时进度条、最多 3 条 + 「还有 N 条」、悬停暂停、点击直达。
 */
export default function TipsStack() {
  return (
    <div style={{ padding: 12, display: 'flex', flexDirection: 'column', gap: 10, background: 'transparent' }}>
      <div className="info" style={{ borderLeft: '3px solid var(--primary)', borderRadius: 8 }}>
        <b>收到文件：第 3 章课件.pdf</b>
        <div style={{ color: 'var(--sub)', fontSize: 'var(--fs-12)', marginTop: 4 }}>来自 教师 · 2.4 MB</div>
      </div>
      <div className="info warn" style={{ borderRadius: 8 }}>还有 2 条新共享</div>
    </div>
  )
}
