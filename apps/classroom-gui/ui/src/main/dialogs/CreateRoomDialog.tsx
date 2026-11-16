/**
 * B2 创建本节课：房间码（可换）、PIN 开关、允许学员自动发现、后台服务状态；
 * 「开始上课」→ configureRoom（FR-T-01）。首次启动由 App 触发，也可从设置再次进入。
 */
import { useState } from 'react'
import Modal from '../../shared/components/Modal'
import { useMainStore } from '../store'

function randomRoom() {
  return `XK-${100 + Math.floor(Math.random() * 900)}`
}

export default function CreateRoomDialog() {
  const open = useMainStore((s) => s.dialog.createRoom)
  const closeDialog = useMainStore((s) => s.closeDialog)
  const configureRoom = useMainStore((s) => s.configureRoom)
  const status = useMainStore((s) => s.status)
  const [room, setRoom] = useState(randomRoom)
  const [pinOn, setPinOn] = useState(true)
  const [pin, setPin] = useState('')
  const [discovery, setDiscovery] = useState(true)

  if (!open) return null

  const submit = async () => {
    await configureRoom(room.trim() || randomRoom(), pinOn ? pin.trim() || '0000' : null, discovery)
  }

  return (
    <Modal
      title={
        <span>
          创建本节课 <span className="chip pri">教师端 · 服务端</span>
        </span>
      }
      onClose={() => closeDialog('createRoom')}
      footer={
        <>
          <button className="btn" onClick={() => closeDialog('createRoom')}>
            取消
          </button>
          <button className="btn pri" onClick={() => void submit()}>
            开始上课
          </button>
        </>
      }
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <div>
          <div style={{ fontSize: 12, color: 'var(--sub)', marginBottom: 4 }}>房间码（局域网内自动广播）</div>
          <div style={{ fontSize: 19, fontWeight: 600, letterSpacing: 2, color: 'var(--primary-d)' }}>{room}</div>
        </div>
        <button className="btn" onClick={() => setRoom(randomRoom())}>
          换一个
        </button>
      </div>
      <div className="srow">
        <span>访问 PIN（防同网段误连）</span>
        <span style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          {pinOn ? (
            <input
              className="input"
              style={{ width: 90 }}
              value={pin}
              onChange={(e) => setPin(e.target.value)}
              placeholder="0000"
            />
          ) : null}
          <button className="btn" onClick={() => setPinOn(!pinOn)}>
            {pinOn ? '已开启' : '已关闭'}
          </button>
        </span>
      </div>
      <div className="srow">
        <span>允许学员自动发现</span>
        <button className="btn" onClick={() => setDiscovery(!discovery)}>
          {discovery ? '已开启' : '已关闭'}
        </button>
      </div>
      <div className="srow">
        <span>后台服务</span>
        <span className={status?.running ? 'chip ok' : 'chip warn'}>
          {status?.running ? '● 已启动' : '● 未运行'}
        </span>
      </div>
      <div className="info pri" style={{ marginTop: 12 }}>
        仅局域网，不接入云端；教师机持有全量权威数据。
      </div>
    </Modal>
  )
}
