/**
 * 「粘贴文本并共享」标题回退规则（纯函数，供对话框与单测复用）。
 *
 * 需求：标题可选；留空时取正文前 10 个字符（按 Unicode 码点计，避免截断 emoji）。
 */
export const TITLE_FALLBACK_CHARS = 10

export function effectiveTitle(title: string, body: string): string {
  const t = title.trim()
  if (t) return t
  return Array.from(body.trim()).slice(0, TITLE_FALLBACK_CHARS).join('')
}
