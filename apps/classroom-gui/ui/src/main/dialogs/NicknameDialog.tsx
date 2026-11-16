/**
 * B1 学员首次设置昵称（FR-S-01）：昵称（1-20 字）+ 检查可用 + 自动发现的房间列表（mock XK-204，
 * T7 后接 SvcNotify::Discovered）+ PIN 输入 + 重名文案（PRD 8.3 逐字）+ 「进入课堂」。
 * 首次必须完成：不可关闭（Modal onClose 为空操作）。
 */
import { useState } from 'react'
import Modal from '../../shared/components/Modal'
import { useMainStore } from '../store'

/** mock 发现列表（T7 Discovered 接线后替换） */
const DISCOVERED = [{ room: 'XK-204', host: '教师机', pinRequired: true }]

export default function NicknameDialog() {
  const open = useMainStore((s) => s.dialog.nickname)
  const setNickname = useMainStore((s) => s.setNickname)
  const [nickname, setNicknameValue] = useState('')
  const [pin, setPin] = useState('')
  const [check, setCheck] = useState<{ ok: boolean; msg: string } | null>(null)
  const [err, setErr] = useState<string | null>(null)

  if (!open) return null

  const room = DISCOVERED[0]

  const doCheck = async () => {
    const name = nickname.trim()
    if (!name || name.length > 20) {
      setCheck({ ok: false, msg: '昵称需为 1 至 20 个字符' })
      return
    }
    // mock 查重（黑名单与 client 一致）
    const taken = name === '小林'
    setCheck(
      taken
        ? { ok: false, msg: '该昵称已被使用，请换一个' }
        : { ok: true, msg: `昵称可用 · 已自动发现教师机「${room.room}」` },
    )
  }

  const submit = async () => {
    const e = await setNickname(nickname.trim(), room.pinRequired ? pin : null)
    setErr(e)
  }

  return (
    <Modal
      title={
        <span>
          欢迎使用课堂共享 <span className="chip pri">学员端</span>
        </span>
      }
      onClose={() => {
        /* 首次必须设置昵称（PRD 6.1）：不可关闭 */
      }}
      footer={
        <>
          <button className="btn pri" onClick={() => void submit()}>
            进入课堂
          </button>
        </>
      }
    >
      <div className="field">
        <label>设置一个昵称，教师端将按昵称对提交内容分类</label>
        <div style={{ display: 'flex', gap: 8 }}>
          <input
            className="input"
            style={{ flex: 1 }}
            value={nickname}
            onChange={(e) => {
              setNicknameValue(e.target.value)
              setCheck(null)
            }}
            placeholder="如：小周"
          />
          <button className="btn" onClick={() => void doCheck()}>
            检查可用
          </button>
        </div>
      </div>
      {check ? (
        <div className={`info ${check.ok ? 'ok' : 'err'}`} style={{ marginBottom: 12 }}>
          {check.msg}
        </div>
      ) : null}
      {room.pinRequired ? (
        <div className="field">
          <label>房间 PIN（教师机 {room.host}「{room.room}」已开启 PIN）</label>
          <input
            className="input"
            style={{ width: 140 }}
            value={pin}
            onChange={(e) => setPin(e.target.value)}
            placeholder="如：2048"
          />
        </div>
      ) : null}
      <div className="srow">
        <span>本机角色</span>
        <span className="chip pri">学员 · 仅教师可见我的共享</span>
      </div>
      {err ? (
        <div className="info err" style={{ marginTop: 10 }}>
          {err}
        </div>
      ) : null}
    </Modal>
  )
}
