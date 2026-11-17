/**
 * 列表列宽与行高共享常量（issue #90）。
 *
 * 虚拟滚动依赖「行高 = CSS 高度 = Rust/JS 常量」三者一致：两个视图各自维护一份
 * 常量容易漂移，统一收口在此。改动时同步 `main.css` 的 `.grow` 高度来源。
 */

/** 单行高度（px）；VirtualList 的 rowHeight 与占位高度均以此为准 */
export const ROW_HEIGHT = 40

/** C1/D2「我共享的」六列：类型 / 名称·内容 / 大小 / 时间 / 状态 / 操作 */
export const COLS_MY_SHARES = '76px minmax(0,1fr) 90px 70px 110px 150px'

/** D1「教师共享」只读五列：类型 / 名称·内容 / 大小 / 时间 / 状态 */
export const COLS_TEACHER_SHARES = '76px minmax(0,1fr) 90px 70px 110px'
