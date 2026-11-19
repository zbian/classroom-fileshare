/**
 * E1 粘贴文本并共享：标题可选（留空取正文前 10 个字符，同名按标题覆盖）、
 * 内容+字数、重新粘贴/清空、接收对象=全体学员（教师端固定）；上限 10000 字（PRD 5.2）。
 */
import { useEffect, useState } from 'react'
import Modal from '../../shared/components/Modal'
import { effectiveTitle } from '../../shared/pasteTitle'
import { useMainStore } from '../store'

const MAX_CHARS = 10_000

export default function PasteTextDialog() {
  const open = useMainStore((s) => s.dialog.pasteText)
  const closeDialog = useMainStore((s) => s.closeDialog)
  const sendText = useMainStore((s) => s.sendText)
  const role = useMainStore((s) => s.role)
  const pasteBody = useMainStore((s) => s.pasteBody)
  const [title, setTitle] = useState('')
  const [body, setBody] = useState('')
  const [err, setErr] = useState<string | null>(null)

  // intent 预填：gui://paste-text 携带的剪贴板内容（右键/拖放/粘贴路径）
  useEffect(() => {
    if (open) setBody(pasteBody ?? '')
  }, [open, pasteBody])

  if (!open) return null

  const repaste = async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const st = await invoke<{ text: string | null }>('read_clipboard_state')
      const text = st.text ?? ''
      if (text.trim()) {
        setBody(text)
        setErr(null)
      } else {
        setErr('剪贴板中没有可共享的文本')
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : '读取剪贴板失败')
    }
  }

  const submit = async () => {
    if (!body.trim()) {
      setErr('内容不能为空白')
      return
    }
    if (body.length > MAX_CHARS) {
      setErr(`超出 ${MAX_CHARS} 字上限`)
      return
    }
    setErr(null)
    await sendText(effectiveTitle(title, body), body)
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
        <label>标题（可选；留空取正文前 10 个字符，同名文本按标题覆盖）</label>
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
        <button className="btn" onClick={() => void repaste()}>
          重新粘贴
        </button>
        <button className="btn" onClick={() => setBody('')}>
          清空
        </button>
        <span style={{ flex: 1 }} />
        <span className="chip pri">接收对象：{role === 'student' ? '仅教师' : '全体学员'}</span>
      </div>
      {err ? (
        <div className="info err" style={{ marginTop: 10 }}>
          {err}
        </div>
      ) : null}
    </Modal>
  )
}
