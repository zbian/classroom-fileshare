/**
 * 「待删除」条目的统一判定与提示文案（issue #106 / PRD 7.4）。
 *
 * 后端 #17：教师删除后本机收到墓碑 `deleted=1` + `sync_state=DeleteDeferred`，
 * 补删完成前条目仍会出现在列表里，但**不可打开、不可复制**。
 *
 * 收口到一处，避免「行内已禁用、工具栏却仍可点」的入口遗漏（review 发现）：
 * 行内 ops（MySharesTab）与工具栏按钮（App.tsx Toolbar）共用本模块。
 */
import type { ShareItem } from '../shared/ipc/types'

/** 是否处于「待删除」锁定态（墓碑或延迟删除） */
export function isItemLocked(item: Pick<ShareItem, 'deleted' | 'sync_state'> | null | undefined): boolean {
  if (!item) return false
  return item.deleted === true || item.sync_state === 'DeleteDeferred'
}

/** 锁定态统一提示（行内 disabled 与工具栏 disabled 的 title 共用） */
export const LOCKED_HINT = '待删除：教师已删除该内容，本机正在等待补删，暂不可打开或复制'
