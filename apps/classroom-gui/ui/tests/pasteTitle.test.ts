import test from 'node:test'
import assert from 'node:assert/strict'

import { effectiveTitle, TITLE_FALLBACK_CHARS } from '../src/shared/pasteTitle.ts'

test('标题非空时使用输入标题（去首尾空格）', () => {
  assert.equal(effectiveTitle('  第 3 章重点  ', '正文内容'), '第 3 章重点')
})

test('标题为空时取正文前 10 个字符', () => {
  const body = '本章重点：1. 掌握共享链路的三层结构；2. 理解同名覆盖'
  const out = effectiveTitle('', body)
  assert.equal(out, '本章重点：1. 掌握')
  assert.equal(Array.from(out).length, TITLE_FALLBACK_CHARS)
})

test('标题为纯空白时同样回退', () => {
  assert.equal(effectiveTitle('   \t', 'abcdefghijKLMN'), 'abcdefghij')
})

test('正文以换行开头时先 trim 再截取', () => {
  assert.equal(effectiveTitle('', '\n\nabcdefghijk'), 'abcdefghij')
})

test('按码点截取，emoji 不被截断', () => {
  const body = '😀😀😀😀😀😀😀😀😀😀😀tail'
  const out = effectiveTitle('', body)
  assert.equal(out, '😀😀😀😀😀😀😀😀😀😀')
  assert.equal(Array.from(out).length, TITLE_FALLBACK_CHARS)
})

test('正文全空白时返回空串（由调用方报「内容不能为空白」）', () => {
  assert.equal(effectiveTitle('', '   \n  '), '')
})

test('正文字符不足 10 个时全部返回', () => {
  assert.equal(effectiveTitle('', 'abc'), 'abc')
})
