/**
 * 轻量虚拟滚动（issue #90；PRD 9.1：500 条首屏 ≤1s，启用虚拟滚动）。
 *
 * 只渲染可视区 + 上下 overscan；占位高度用总条数 × 行高，保证滚动条比例正确、
 * 滚到顶/底不错位（不依赖任何第三方库，保持 GUI 包体与内存预算）。
 */
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'

export interface VirtualListProps<T> {
  items: T[]
  /** 行高（px），必须与 CSS 保持一致 */
  rowHeight: number
  /** 可视区上下额外渲染的行数，滚动时防白边 */
  overscan?: number
  renderRow: (item: T, index: number) => ReactNode
  /** 视口类名（占用父容器剩余高度） */
  className?: string
}

export default function VirtualList<T>({
  items,
  rowHeight,
  overscan = 8,
  renderRow,
  className,
}: VirtualListProps<T>) {
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [viewportHeight, setViewportHeight] = useState(0)

  // 视口高度：首帧布局后测一次，窗口/容器尺寸变化时同步（避免依赖滚动事件）
  useLayoutEffect(() => {
    const el = viewportRef.current
    if (!el) return
    const update = () => setViewportHeight(el.clientHeight)
    update()
    const observer = new ResizeObserver(update)
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  // 列表变短（如筛选后）时把滚动位置拉回有效范围，避免出现空白视口
  useEffect(() => {
    const maxTop = Math.max(0, items.length * rowHeight - viewportHeight)
    if (scrollTop > maxTop) {
      const next = Math.min(scrollTop, maxTop)
      setScrollTop(next)
      if (viewportRef.current) viewportRef.current.scrollTop = next
    }
  }, [items.length, rowHeight, viewportHeight, scrollTop])

  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan)
  const end = Math.min(
    items.length,
    Math.ceil((scrollTop + Math.max(viewportHeight, rowHeight)) / rowHeight) + overscan,
  )
  const visible = items.slice(start, end)

  return (
    <div
      ref={viewportRef}
      className={className ?? 'vlist-viewport'}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
    >
      <div className="vlist-inner" style={{ height: items.length * rowHeight }}>
        <div style={{ transform: `translateY(${start * rowHeight}px)` }}>
          {visible.map((item, i) => renderRow(item, start + i))}
        </div>
      </div>
    </div>
  )
}
