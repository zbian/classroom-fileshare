/**
 * 浮动图标窗（icon.html，Task 8：A1 常态 / A3 右键菜单 / A4 投放态）。
 *
 * 三态由 Rust 侧窗口尺寸承载（命中区=可见区），前端按事件切换视觉：
 * - 常态：72×72 圆形，data-tauri-drag-region 拖动，双击开主窗
 * - 投放态：Rust DragDrop 事件驱动 124×124 放大（gui://icon-drag）
 * - 菜单态：右键触发 open_icon_menu（Rust 依工作区翻转），blur/Esc/选择收起
 */
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useState } from 'react'
import ContextMenu, { type MenuItem } from './ContextMenu'
import { droppedFilesPayload } from '../shared/dnd'

/** 右键菜单展开方向（Rust open_icon_menu 依屏幕工作区计算） */
interface MenuLayout {
  flipX: boolean
  flipY: boolean
}

interface IconStateDto {
  pausedTips: boolean
}

export default function IconApp() {
  const [menuOpen, setMenuOpen] = useState(false)
  const [layout, setLayout] = useState<MenuLayout>({ flipX: false, flipY: false })
  const [dragOver, setDragOver] = useState(false)
  // A4：拖入内容数量（文件/目录），用于投放态反馈
  const [dropCounts, setDropCounts] = useState({ files: 0, dirs: 0 })
  const [paused, setPaused] = useState(false)
  const [pasteAvailable, setPasteAvailable] = useState(false)
  const [pasteHint, setPasteHint] = useState('')

  const closeMenu = useCallback(async () => {
    setMenuOpen(false)
    try {
      await invoke('close_icon_menu')
    } catch {
      // 窗口可能已复位，忽略
    }
  }, [])

  const openMenu = useCallback(async () => {
    try {
      setLayout(await invoke<MenuLayout>('open_icon_menu'))
      setMenuOpen(true)
      // T12「粘贴并共享」三态：探测剪贴板（EX-03 空置灰/占用提示）
      const st = await invoke<{ text: string | null; files: string[]; dirs: number }>(
        'read_clipboard_state',
      )
      const has = (st.files?.length ?? 0) > 0 || !!st.text?.trim()
      setPasteAvailable(has)
      setPasteHint(has ? '' : '剪贴板中没有可共享的文本或文件')
    } catch (e) {
      setMenuOpen(true)
      setPasteAvailable(false)
      setPasteHint(String(e).slice(0, 30))
    }
  }, [])

  useEffect(() => {
    const unlistenBlur = listen('gui://icon-blur', () => {
      void closeMenu()
    })
    const unlistenDrag = listen<{ active: boolean; files: number; dirs: number }>(
      'gui://icon-drag',
      (e) => {
        setDragOver(e.payload.active)
        setDropCounts({ files: e.payload.files, dirs: e.payload.dirs })
      },
    )
    void invoke<IconStateDto>('get_icon_state').then((s) => setPaused(s.pausedTips))
    // 前端就绪：通知 Rust 显示窗口（visible:false 创建，防白闪）
    void invoke('icon_ready')
    return () => {
      void unlistenBlur.then((f) => f())
      void unlistenDrag.then((f) => f())
    }
  }, [closeMenu])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void closeMenu()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [closeMenu])

  // HTML5 拖放（原生 drag-drop 已关闭）：文件 → 落盘后发送；文本 → 打开主窗粘贴对话框
  useEffect(() => {
    let depth = 0
    const onDragEnter = (e: DragEvent) => {
      e.preventDefault()
      depth += 1
      setDragOver(true)
    }
    const onDragOver = (e: DragEvent) => {
      e.preventDefault()
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'
    }
    const onDragLeave = () => {
      depth = Math.max(0, depth - 1)
      if (depth === 0) setDragOver(false)
    }
    const onDrop = (e: DragEvent) => {
      e.preventDefault()
      depth = 0
      setDragOver(false)
      const dt = e.dataTransfer
      if (!dt) return
      const files = dt.files
      if (files && files.length > 0) {
        void (async () => {
          try {
            const payload = await droppedFilesPayload(files)
            await invoke<number>('send_dropped_files', { files: payload })
          } catch (err) {
            setPasteHint(String(err).slice(0, 20))
          }
        })()
        return
      }
      const text = dt.getData('text/plain')
      if (text.trim()) {
        void invoke('open_main_paste', { body: text })
      }
    }
    document.addEventListener('dragenter', onDragEnter)
    document.addEventListener('dragover', onDragOver)
    document.addEventListener('dragleave', onDragLeave)
    document.addEventListener('drop', onDrop)
    return () => {
      document.removeEventListener('dragenter', onDragEnter)
      document.removeEventListener('dragover', onDragOver)
      document.removeEventListener('dragleave', onDragLeave)
      document.removeEventListener('drop', onDrop)
    }
  }, [])

  const onCircleMouseDown = (e: React.MouseEvent) => {
    // drag-region 下原生 dblclick 不可靠，用 mousedown 的 detail 判定双击（FR-G-04）
    if (e.button === 0 && e.detail >= 2) {
      void invoke('open_main')
      void closeMenu()
    }
  }

  const onCircleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    if (!dragOver) void openMenu()
  }

  const items: MenuItem[] = [
    // T12：剪贴板三态接通（文件直发链路 / 文本 E1 预填确认；空/占用置灰带文案）
    {
      key: 'paste-share',
      label: '粘贴并共享',
      hint: pasteAvailable ? 'Ctrl+V' : pasteHint.slice(0, 14),
      disabled: !pasteAvailable,
      onSelect: () => {
        void invoke('paste_and_share')
          .then(() => void closeMenu())
          .catch((e) => setPasteHint(String(e).slice(0, 14)))
      },
    },
    { key: 'paste-draft', label: '粘贴为文本草稿…', disabled: true },
    { key: 'sep-1', label: '', separator: true },
    {
      key: 'open',
      label: '打开「课堂共享」',
      hint: '双击同效',
      onSelect: () => {
        void invoke('open_main')
        void closeMenu()
      },
    },
    {
      key: 'pause',
      label: paused ? '恢复接收提醒' : '暂停接收提醒',
      onSelect: () => {
        void invoke<boolean>('set_pause_tips', { paused: !paused }).then(setPaused)
        void closeMenu()
      },
    },
    { key: 'sep-2', label: '', separator: true },
    { key: 'settings', label: '设置', disabled: true },
    {
      key: 'quit',
      label: '退出课堂共享',
      danger: true,
      onSelect: () => {
        void invoke('gui_quit')
      },
    },
  ]

  if (dragOver) {
    const { files, dirs } = dropCounts
    return (
      <div className="icon-root drop">
        <div className="float-circle drop-zone" data-tauri-drag-region="">
          <span className="drop-main">松手即共享</span>
          {files > 0 ? <span className="drop-sub">{files} 个文件</span> : null}
          {dirs > 0 ? <span className="drop-sub warn">{dirs} 个目录将跳过</span> : null}
          {files === 0 && dirs === 0 ? <span className="drop-sub">文件 / 文本</span> : null}
        </div>
      </div>
    )
  }

  const circle = (
    <div
      className="float-circle"
      data-tauri-drag-region=""
      title="课堂共享：双击打开 · 右键菜单 · 可拖动"
      onMouseDown={onCircleMouseDown}
      onContextMenu={onCircleContextMenu}
    >
      共享
    </div>
  )

  if (!menuOpen) {
    return <div className="icon-root">{circle}</div>
  }

  return (
    <div
      className={`icon-root menu${layout.flipX ? ' flip-x' : ''}${layout.flipY ? ' flip-y' : ''}`}
    >
      <div className="icon-cell">{circle}</div>
      <div className="menu-cell">
        <ContextMenu items={items} />
      </div>
    </div>
  )
}
