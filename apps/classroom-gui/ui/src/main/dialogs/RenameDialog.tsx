/**
 * E4 重命名对话框（M2 #19，PRD 5.4）：预填当前标题 → 提交 `RenameItem`。
 *
 * 错误来源：
 * 1) 本地预检（renameRules.ts，镜像服务端 sanitize/risky-ext/同名规则）→ 行内红字
 * 2) 服务端最终结果：由于 #126 的 RenameItem 是命令下行（fire-and-forget），失败只记日志；
 *    提交后由 store 轮询确认（名称是否生效），未生效则提示「重命名失败：名称冲突或不合法」。
 */
import { useEffect, useRef, useState } from 'react'
import Modal from '../../shared/components/Modal'
import { useMainStore } from '../store'

export default function RenameDialog() {
  const targetId = useMainStore((s) => s.dialog.rename)
  const item = useMainStore((s) => s.items.find((i) => i.id === s.dialog.rename))
  const closeDialog = useMainStore((s) => s.closeDialog)
  const renameItem = useMainStore((s) => s.renameItem)
  const [value, setValue] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const inputRef = useRef<HTMLInputElement | null>(null)

  // 打开时预填当前标题并全选（便于直接覆盖输入）
  useEffect(() => {
    if (targetId && item) {
      setValue(item.title)
      setErr(null)
      setBusy(false)
      window.setTimeout(() => inputRef.current?.select(), 0)
    }
  }, [targetId, item?.id])

  if (!targetId || !item) return null

  const submit = async () => {
    if (busy) return
    setBusy(true)
    setErr(null)
    const message = await renameItem(targetId, value)
    setBusy(false)
    if (message) setErr(message)
  }

  return (
    <Modal
      title="重命名"
      onClose={() => !busy && closeDialog('rename')}
      footer={
        <>
          <button className="btn" disabled={busy} onClick={() => closeDialog('rename')}>
            取消
          </button>
          <button className="btn pri" disabled={busy} onClick={() => void submit()}>
            {busy ? '提交中…' : '确认'}
          </button>
        </>
      }
    >
      <div className="field">
        <label>新名称（文件会同步重命名并广播给学员端）</label>
        <input
          ref={inputRef}
          className="input"
          value={value}
          autoFocus
          disabled={busy}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void submit()
          }}
        />
      </div>
      {err ? (
        <div className="info err" style={{ marginTop: 10 }} role="alert">
          {err}
        </div>
      ) : null}
    </Modal>
  )
}
