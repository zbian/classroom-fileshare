/**
 * HTML5 拖放工具：把浏览器 File 读成 base64，交给 Rust 落临时文件后走 SendFiles。
 *
 * 背景（#拖拽修复）：Windows 上若启用 Tauri 原生 drag-drop，WebView 收不到 HTML5
 * drop 事件（纯文本拖放无法支持）；因此关闭原生 drag-drop，改由前端统一处理
 * 文件 + 文本两种拖放。WebView 拿不到文件绝对路径，故以字节回传、Rust 落盘。
 *
 * 已知约束（评审 B1）：base64 链路内存峰值约为文件大小 4.3 倍，且 IPC 走 JSON。
 * 因此拖放前必须做大小预检（与服务端 EX-04 的 100 MB 单文件上限一致）。
 */
export const MAX_DROP_FILE_BYTES = 100 * 1024 * 1024
export const MAX_DROP_TOTAL_BYTES = 200 * 1024 * 1024
export const MAX_DROP_FILES = 50

/** 拖放前置校验：返回错误文案；null 表示可发送。 */
export function validateDroppedFiles(files: FileList | File[]): string | null {
  const list = Array.from(files)
  if (list.length > MAX_DROP_FILES) {
    return `单次最多拖放 ${MAX_DROP_FILES} 个文件`
  }
  let total = 0
  for (const f of list) {
    if (f.size > MAX_DROP_FILE_BYTES) {
      return `「${f.name}」超过单文件 100 MB 上限，请改用「添加文件」或压缩后发送`
    }
    total += f.size
  }
  if (total > MAX_DROP_TOTAL_BYTES) {
    return `单次拖放总大小超过 200 MB，请分批发送`
  }
  return null
}

/** 拖放/粘贴是否发生在可编辑控件上（交给浏览器原生处理）。 */
export function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null
  if (!el) return false
  const tag = el.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || el.isContentEditable === true
}

export async function fileToBase64(file: File): Promise<string> {
  const buf = await file.arrayBuffer()
  const bytes = new Uint8Array(buf)
  let binary = ''
  const chunk = 0x8000 // 0x8000 是 String.fromCharCode 展开的安全上限附近；调大可能超参数上限
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

/** 把 dataTransfer 中的文件转成 Rust 命令入参。 */
export async function droppedFilesPayload(
  files: FileList | File[],
): Promise<Array<{ name: string; dataBase64: string }>> {
  const list = Array.from(files)
  const out: Array<{ name: string; dataBase64: string }> = []
  for (const f of list) {
    out.push({ name: f.name, dataBase64: await fileToBase64(f) })
  }
  return out
}
