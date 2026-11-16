/**
 * E1 粘贴文本并共享：标题必填（同名按标题覆盖）、内容+字数、重新粘贴/清空、
 * 接收对象=全体学员（教师端固定）；上限 10000 字（PRD 5.2）。
 */
import { useState } from 'react'
import Modal from '../../shared/components/Modal'
import { useMainStore } from '../store'

const MAX_CHARS = 10_000
const SAMPLE = '本章重点：1. 掌握共享链路的三层结构；2. 理解同名覆盖的作用域键；3. 下课前完成 P42 练习并回传。'

export default function PasteTextDialog() {
  const open = useMainStore((s) => s.dialog.pasteText)
  const closeDialog = useMainStore((s) => s.closeDialog)
  const sendText = useMainStore((s) => s.sendText)
  const [title, setTitle] = useState('')
  const [body, setBody] = useState('')
  const [err, setErr] = useState<string | null>(null)

  if (!open) return null

  const repaste = () => {
    // T12 接剪贴板插件；dev 暂以样例文本模拟剪贴板内容
    setBody(SAMPLE)
  }

  const submit = async () => {
    if (!title.trim()) {
      setErr('标题必填（同名文本按标题覆盖）')
      return
    }
    if (!body.trim()) {
      setErr('内容不能为空白')
      return
    }
    if (body.length > MAX_CHARS) {
      setErr(`超出 ${MAX_CHARS} 字上限`)
      return
    }
    setErr(null)
    await sendText(title.trim(), body)
  }

  return (
    <Modal
      title="粘贴文本并共享"
      onClose={() => closeDialog('pasteText')}
      footer={
        <>
          <button className="btn" onClick={() => closeDialog('pasteText')}>
            取消
          </button>
          <button className="btn pri" onClick={() => void submit()}>
            共享
          </button>
        </>
      }
    >
      <div className="field">
        <label>标题（必填，同名文本按标题覆盖）</label>
        <input className="input" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="如：第 3 章重点" />
      </div>
      <div className="field">
        <label>
          内容{body ? ` · ${body.length} 字` : '（点击「重新粘贴」读取剪贴板）'}
        </label>
        <textarea
          className="textarea"
          value={body}
          onChange={(e) => setBody(e.target.value)}
          placeholder="粘贴或输入要共享的文本…"
        />
      </div>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <button className="btn" onClick={repaste}>
          重新粘贴
        </button>
        <button className="btn" onClick={() => setBody('')}>
          清空
        </button>
        <span style={{ flex: 1 }} />
        <span className="chip pri">接收对象：全体学员</span>
      </div>
      {err ? (
        <div className="info err" style={{ marginTop: 10 }}>
          {err}
        </div>
      ) : null}
    </Modal>
  )
}
