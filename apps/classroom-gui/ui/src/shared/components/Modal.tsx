/**
 * 三段式弹窗基座（PRD 8.2：标题 / 内容 / 底部按钮；危险操作红钮与取消并列右对齐）。
 * 所有 E1/E2/E4/B1/B2/F1 弹窗复用此组件。
 */
import { useEffect, type ReactNode } from 'react'

export interface ModalProps {
  title: ReactNode
  children: ReactNode
  /** 底部按钮区（由调用方给出，含取消/确认） */
  footer?: ReactNode
  onClose: () => void
  width?: number
}

export default function Modal({ title, children, footer, onClose, width = 460 }: ModalProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div className="modal-overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" style={{ width }} role="dialog" aria-modal>
        <div className="modal-hd">
          <span>{title}</span>
          <button className="modal-x" aria-label="关闭" onClick={onClose}>
            ×
          </button>
        </div>
        <div className="modal-bd">{children}</div>
        {footer ? <div className="modal-ft">{footer}</div> : null}
      </div>
    </div>
  )
}
