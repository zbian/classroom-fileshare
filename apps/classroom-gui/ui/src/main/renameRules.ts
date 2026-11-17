/**
 * 重命名校验（M2 #19；GUI 侧预检，规则逐条镜像服务端）：
 *
 * - `crates/protocol/src/item.rs::sanitize_file_name`：剥路径、拒绝空/`.`/`..`、
 *   尾随 `.`/空格、NTFS 保留设备名、Windows 非法字符（`< > : " | ? *` 与控制字符）
 * - `crates/protocol/src/item.rs::is_risky_ext` + `RISKY_EXTS`：文件高风险扩展名（PRD 5.3 / EX-05）
 * - `crates/store` `rename_item`：同作用域同名 → `NameTaken`（M2 拒绝、不静默覆盖，PRD 5.4 覆盖语义留 M3）
 *
 * 为什么要在 GUI 预检：#126 的 `GuiRequest::RenameItem` 是**命令下行**（fire-and-forget），
 * 服务端失败只记日志不回传错误；GUI 只有在发送前挡住可预知错误 + 发送后校验生效，
 * 才能给出明确中文提示。服务端仍是最终权威（并发/竞态仍由 store 兜底）。
 */

/** 高风险扩展名（与 Rust `RISKY_EXTS` 保持一致；改动需同步两处） */
export const RISKY_EXTS = ['exe', 'bat', 'cmd', 'com', 'scr', 'pif', 'vbs', 'js', 'jar', 'lnk']

/** NTFS 保留设备名（与 Rust `RESERVED_DEVICE_NAMES` 一致） */
export const RESERVED_DEVICE_NAMES = [
  'con', 'prn', 'aux', 'nul',
  'com1', 'com2', 'com3', 'com4', 'com5', 'com6', 'com7', 'com8', 'com9',
  'lpt1', 'lpt2', 'lpt3', 'lpt4', 'lpt5', 'lpt6', 'lpt7', 'lpt8', 'lpt9',
]

/** Windows 非法字符（与 Rust 判定一致） */
const ILLEGAL_CHARS = '<>:"|?*'

/** 是否 NTFS 保留设备名（大小写不敏感，含 `CON.txt` 形态） */
export function isReservedDeviceName(name: string): boolean {
  const stem = name.split('.')[0] ?? ''
  return RESERVED_DEVICE_NAMES.includes(stem.toLowerCase())
}

/** 是否高风险扩展名（大小写不敏感，先归一尾随 `.`/空格） */
export function isRiskyExt(name: string): boolean {
  const normalized = name.replace(/[. ]+$/, '')
  const idx = normalized.lastIndexOf('.')
  if (idx < 0) return false
  const ext = normalized.slice(idx + 1).toLowerCase()
  return RISKY_EXTS.includes(ext)
}

/** 服务端语义的净化：只取最后一段文件名（`a\b\c.txt` → `c.txt`） */
export function sanitizeSegment(raw: string): string {
  const parts = raw.split(/[\\/]/)
  return parts[parts.length - 1] ?? ''
}

export interface RenameCheck {
  /** 净化后的新名称（预检通过时用于提交） */
  name: string
  /** 错误文案；null 表示预检通过 */
  error: string | null
}

/**
 * 预检重命名输入。
 * @param raw 用户输入
 * @param kind 条目类型（文件才做高风险扩展名检查）
 * @param selfTitle 当前标题（用于判断「未改动」）
 * @param existingTitles 同作用域其它条目标题（判定同名冲突）
 */
export function checkRename(
  raw: string,
  kind: 'Text' | 'File',
  selfTitle: string,
  existingTitles: string[],
): RenameCheck {
  const name = sanitizeSegment(raw)

  if (name.length === 0) return { name, error: '名称不能为空' }
  if (name === '.' || name === '..') return { name, error: '名称不能为 . 或 ..' }
  if (name.endsWith('.') || name.endsWith(' '))
    return { name, error: '名称不能以空格或句点结尾' }
  if (isReservedDeviceName(name))
    return { name, error: '该名称是 Windows 保留名称（如 CON/NUL），请换一个' }
  for (const ch of name) {
    const code = ch.charCodeAt(0)
    // 与 Rust `char::is_control()` 对齐：C0（<0x20）+ C1（0x7F–0x9F）
    if (ILLEGAL_CHARS.includes(ch) || code < 0x20 || (code >= 0x7f && code <= 0x9f)) {
      return { name, error: '名称不能包含 < > : " | ? * 等字符' }
    }
  }
  if (kind === 'File' && isRiskyExt(name)) {
    return { name, error: '该类型不允许共享（EX-05），请换一个扩展名' }
  }
  if (name !== selfTitle && existingTitles.includes(name)) {
    return { name, error: '该名称已存在，请换一个' }
  }
  return { name, error: null }
}
