/**
 * 单条 tips 气泡（A2）：名称 / 来源·大小 / 15s 倒计时进度条。
 * 点击直达主窗对应条目（open_main_locate → T9 消费 gui://locate）。
 */
import { invoke } from '@tauri-apps/api/core'

export interface TipCardProps {
  id: string
  kind: string
  name: string
  from: string
  sizeText: string
  remainMs: number
  totalMs: number
}

export default function TipCard({ id, kind, name, from, sizeText, remainMs, totalMs }: TipCardProps) {
  const pct = Math.max(0, Math.min(100, (remainMs / totalMs) * 100))
  return (
    <div
      className="tip-card"
      title="点击打开对应条目"
      onClick={() => void invoke('open_main_locate', { id })}
    >
      <div className="ttl">
        {kind === 'file' ? '收到文件：' : '收到文本：'}
        {name}
      </div>
      <div className="ss">
        <span>
          来自 {from} · {sizeText}
        </span>
        <span>{Math.ceil(remainMs / 1000)}s</span>
      </div>
      <div className="bar">
        <i style={{ width: `${pct}%` }} />
      </div>
    </div>
  )
}
