/** 自绘右键菜单（A3 六项）。样式见 icon.css 的 .ctx-* */
export interface MenuItem {
  key: string
  label: string
  /** 右侧弱提示（如 Ctrl+V、双击同效） */
  hint?: string
  /** 占位禁用态（链路未接通的菜单项） */
  disabled?: boolean
  /** 危险操作红字（退出课堂共享） */
  danger?: boolean
  separator?: boolean
  onSelect?: () => void
}

export default function ContextMenu({ items }: { items: MenuItem[] }) {
  return (
    <div className="ctx-menu" role="menu">
      {items.map((it) =>
        it.separator ? (
          <div className="ctx-sep" key={it.key} />
        ) : (
          <div
            key={it.key}
            role="menuitem"
            aria-disabled={it.disabled ?? false}
            className={['ctx-item', it.disabled ? 'disabled' : '', it.danger ? 'danger' : '']
              .join(' ')
              .trim()}
            onClick={it.disabled ? undefined : it.onSelect}
          >
            <span>{it.label}</span>
            {it.hint ? <small>{it.hint}</small> : null}
          </div>
        ),
      )}
    </div>
  )
}
