/**
 * E4 删除确认：PRD 8.3 逐字文案 + 红色确认钮与取消并列右对齐。
 * M1 简化为确认+完成态；逐台同步进度（24/28 台）M2 接 DeleteProgress。
 */
import Modal from '../../shared/components/Modal'
import { useMainStore } from '../store'

export default function DeleteConfirmDialog() {
  const pendingId = useMainStore((s) => s.dialog.deleteConfirm)
  const item = useMainStore((s) => s.items.find((i) => i.id === s.dialog.deleteConfirm))
  const closeDialog = useMainStore((s) => s.closeDialog)
  const confirmDelete = useMainStore((s) => s.confirmDelete)

  if (!pendingId) return null

  return (
    <Modal
      title={<span style={{ color: 'var(--red)' }}>删除共享内容</span>}
      onClose={() => closeDialog('deleteConfirm')}
      footer={
        <>
          <button className="btn" onClick={() => closeDialog('deleteConfirm')}>
            取消
          </button>
          <button
            className="btn"
            style={{ background: 'var(--red)', borderColor: 'var(--red)', color: '#fff' }}
            onClick={() => void confirmDelete()}
          >
            确认删除
          </button>
        </>
      }
    >
      <div className="info err" style={{ marginBottom: 12 }}>
        删除「{item?.title ?? pendingId}」后将从所有学员端同步移除本地副本，此操作不可撤销。
      </div>
      <div className="info">在线学员端将立即删除；离线学员端将在其上线后自动删除，删除完成后列表同步刷新。</div>
    </Modal>
  )
}
