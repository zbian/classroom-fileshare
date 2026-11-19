import test from 'node:test'
import assert from 'node:assert/strict'

import {
  isEditableTarget,
  validateDroppedFiles,
  MAX_DROP_FILE_BYTES,
  MAX_DROP_TOTAL_BYTES,
} from '../src/shared/dnd.ts'

/** 构造只带 size/name 的伪 File（避免真的分配 100MB 内存）。 */
function fakeFile(name: string, size: number): File {
  return { name, size } as unknown as File
}

test('普通文件通过校验', () => {
  assert.equal(validateDroppedFiles([fakeFile('a.txt', 1024)]), null)
})

test('单文件超过 100MB 被拒绝并带文件名', () => {
  const err = validateDroppedFiles([fakeFile('big.zip', MAX_DROP_FILE_BYTES + 1)])
  assert.ok(err && err.includes('big.zip') && err.includes('100 MB'))
})

test('恰好 100MB 通过', () => {
  assert.equal(validateDroppedFiles([fakeFile('edge.bin', MAX_DROP_FILE_BYTES)]), null)
})

test('总量超过 200MB 被拒绝', () => {
  const third = Math.ceil(MAX_DROP_TOTAL_BYTES / 3)
  const err = validateDroppedFiles([
    fakeFile('a.bin', third),
    fakeFile('b.bin', third),
    fakeFile('c.bin', third),
  ])
  assert.equal(typeof err, 'string')
})

test('空文件列表通过（调用方自行忽略）', () => {
  assert.equal(validateDroppedFiles([]), null)
})

test('输入框/文本域/可编辑区被识别为可编辑目标', () => {
  assert.equal(isEditableTarget({ tagName: 'INPUT', isContentEditable: false } as unknown), true)
  assert.equal(isEditableTarget({ tagName: 'TEXTAREA', isContentEditable: false } as unknown), true)
  assert.equal(isEditableTarget({ tagName: 'DIV', isContentEditable: true } as unknown), true)
})

test('普通元素/null 不是可编辑目标', () => {
  assert.equal(isEditableTarget({ tagName: 'DIV', isContentEditable: false } as unknown), false)
  assert.equal(isEditableTarget(null), false)
})
