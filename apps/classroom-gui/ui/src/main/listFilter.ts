/**
 * 列表搜索与类型筛选（issue #90；PRD 5.4 / 9.1）。
 *
 * 纯函数 + 客户端过滤：M2 列表规模（≤ 数百条）一次拉取后本地过滤，避免逐字符
 * 触发 IPC 往返（即时过滤、大小写不敏感、中文子串可用）。
 *
 * 口径说明：
 * - 搜索匹配 `title`（名称）。PRD 5.4 的「内容搜索」需文本正文入列表载荷，
 *   属 M2 后续（正文目前存于仓库文件，不在 ShareItem 内）——issue #90 范围写明
 *   先做名称搜索，此处保持一致。
 * - 类型筛选与搜索为「与」关系；`all` 不过滤。
 */
import type { UiShareItem } from '../shared/ipc/client'

/** 类型筛选星盘：全部 / 文本 / 文件 */
export type KindFilter = 'all' | 'Text' | 'File'

/** 单个 Tab 的筛选状态（按 Tab 独立保存，切换 Tab 不互相污染） */
export interface TabFilter {
  keyword: string
  kind: KindFilter
}

export const EMPTY_FILTER: TabFilter = { keyword: '', kind: 'all' }

/** 是否处于筛选态（空态文案与「筛选后 N 条」提示用） */
export function isFilterActive(filter: TabFilter): boolean {
  return filter.keyword.trim().length > 0 || filter.kind !== 'all'
}

/**
 * 过滤列表。
 * - 关键字：去首尾空白后按 `toLowerCase` 做子串匹配（大小写不敏感；中文不受影响）
 * - 类型：精确匹配 `kind`
 */
export function filterItems(items: UiShareItem[], filter: TabFilter): UiShareItem[] {
  const keyword = filter.keyword.trim().toLowerCase()
  return items.filter((item) => {
    if (filter.kind !== 'all' && item.kind !== filter.kind) return false
    if (!keyword) return true
    return item.title.toLowerCase().includes(keyword)
  })
}
