/**
 * main.html 占位首页（Task 1b）。
 * T9/T10 将按 role 渲染教师视图（C1）与学员视图（D1/D2）的六层结构。
 */
export default function App() {
  const role = document.documentElement.dataset.role ?? 'teacher'
  return (
    <div style={{ padding: 32, display: 'flex', flexDirection: 'column', gap: 12 }}>
      <h1 style={{ fontSize: 'var(--fs-17)' }}>课堂共享 · GUI 脚手架</h1>
      <div style={{ display: 'flex', gap: 8 }}>
        <span className="chip pri">{role === 'student' ? '学员端' : '教师端'}</span>
        <span className="chip">main.html 入口</span>
      </div>
      <div className="info pri">tokens.css 已按 PRD 4.6 生效：主色随 data-role 切换（教师蓝 / 学员青）。</div>
    </div>
  )
}
