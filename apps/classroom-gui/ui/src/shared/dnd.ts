/**
 * HTML5 拖放工具：把浏览器 File 读成 base64，交给 Rust 落临时文件后走 SendFiles。
 *
 * 背景（#拖拽修复）：Windows 上若启用 Tauri 原生 drag-drop，WebView 收不到 HTML5
 * drop 事件（纯文本拖放无法支持）；因此关闭原生 drag-drop，改由前端统一处理
 * 文件 + 文本两种拖放。WebView 拿不到文件绝对路径，故以字节回传、Rust 落盘。
 */
export async function fileToBase64(file: File): Promise<string> {
  const buf = await file.arrayBuffer()
  const bytes = new Uint8Array(buf)
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

/** 把 dataTransfer 中的文件转成 Rust 命令入参。 */
export async function droppedFilesPayload(
  files: FileList | File[],
): Promise<Array<{ name: string; data_base64: string }>> {
  const list = Array.from(files)
  const out: Array<{ name: string; data_base64: string }> = []
  for (const f of list) {
    out.push({ name: f.name, data_base64: await fileToBase64(f) })
  }
  return out
}
