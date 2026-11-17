/**
 * 删除同步进度条（M2 #114 S5 ack 聚合；PRD 5.8「界面展示同步进度（24/28 台已删除）」）。
 *
 * 数据：`SvcNotify::DeleteProgress { item_id, done, total }`（notify retag 统一外壳，
 * store 读 `n.payload` 并收敛 done=min(done,total)）。
 * 展示：进行中「正在同步删除：done/total 台」；完成「已同步删除：total/total 台」保留 2.5s。
 * total<=0（无参与端）不显示——由删除确认框/现有文案承担「离线端上线后自动删除」。
 */
import { useMainStore } from '../store'

export default function DeleteProgressBar() {
  const progress = useMainStore((s) => s.deleteProgress)
  if (!progress || progress.total <= 0) return null

  const done = Math.min(progress.done, progress.total)
  const finished = progress.completed_at !== null
  return (
    <div
      className={`info ${finished ? 'ok' : 'warn'}`}
      role="status"
      aria-live="polite"
      style={{ margin: '6px 16px 0', display: 'flex', alignItems: 'center', gap: 8 }}
    >
      <span style={{ flex: 1 }}>
        {finished ? `已同步删除：${done}/${progress.total} 台` : `正在同步删除：${done}/${progress.total} 台`}
      </span>
      <span className="chip">{finished ? '完成' : '同步中'}</span>
    </div>
  )
}
