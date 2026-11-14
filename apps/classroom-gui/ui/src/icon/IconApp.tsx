/**
 * icon.html 占位（Task 1b）：72px 圆形浮动图标（A1）。
 * T8 实现：置顶窗口、drag-region 拖动、右键菜单（A3）、投放态（A4）、位置持久化。
 */
export default function IconApp() {
  return (
    <div
      style={{
        height: '100%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'transparent',
      }}
    >
      <div className="float-circle">共享</div>
    </div>
  )
}
