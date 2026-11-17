/**
 * tips 气泡堆叠（Task 11：FR-G-02 / A2 / TC-10）。
 *
 * - 队列：完整队列存内存（tips 不落盘）；最多同时显示 3 条，超出合并为
 *   「还有 N 条」（TC-10：连发 5 条 → 3 卡 + 合并行）
 * - 倒计时：15s，仅前 3 条计时；悬停暂停（mouseenter/mouseleave）、移出继续
 * - 窗高随堆叠数经 update_tips_height 同步（命中区=可见区）
 * - 队列清空 → tips_dismissed 隐藏窗口；进程退出即清空（无持久化）
 * - 「暂停接收提醒」：Rust 侧抑制 show，数据仍下发入队（PRD 4.3 只抑制显示）
 */
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef, useState } from 'react'
import TipCard from './TipCard'

const TOTAL_MS = 15_000
const TICK_MS = 100
const MAX_CARDS = 3
/** 与 src-tauri 高度钳制共用的一组布局常量（tips.css 同值） */
const CARD_H = 72
const MORE_H = 36
const GAP = 10
const PAD = 12

interface TipDto {
  id: string
  kind: string
  name: string
  from: string
  sizeText: string
}

interface Tip extends TipDto {
  remainMs: number
}

interface ToastItem {
  id: string
  text: string
}

export default function TipsStack() {
  const [queue, setQueue] = useState<Tip[]>([])
  const [toasts, setToasts] = useState<ToastItem[]>([])
  const [hover, setHover] = useState(false)
  const hoverRef = useRef(false)

  useEffect(() => {
    hoverRef.current = hover
  }, [hover])

  // 接收推送 + dev 演示入口（tips.html?demoTips=N 或 #demo=N，仅 debug 命令生效）
  useEffect(() => {
    const unlisten = listen<TipDto>('svc://share-received', (e) => {
      setQueue((q) => [...q, { ...e.payload, remainMs: TOTAL_MS }])
    })
    // T12：toast 提示（拖放/粘贴共享结果等，桌面可见；3s 消失，无倒计时）
    const unlistenToast = listen<string>('gui://toast', (e) => {
      setToasts((t) => [...t, { id: `${Date.now()}-${Math.random()}`, text: e.payload }])
    })
    const fromQuery = new URLSearchParams(window.location.search).get('demoTips')
    const fromHash = /#demo=(\d+)/.exec(window.location.hash)?.[1]
    const n = Number(fromQuery ?? fromHash ?? 0)
    if (n > 0) void invoke('dev_demo_tips', { n }).catch(() => {})
    return () => {
      void unlisten.then((f) => f())
      void unlistenToast.then((f) => f())
    }
  }, [])

  // toast 3s 自动消失
  useEffect(() => {
    if (toasts.length === 0) return
    const timer = window.setTimeout(() => setToasts((t) => t.slice(1)), 3000)
    return () => window.clearTimeout(timer)
  }, [toasts])

  // 倒计时：悬停整体暂停（TC-10：悬停 30s 不消失），仅显示中的前 3 条计时
  useEffect(() => {
    const timer = window.setInterval(() => {
      if (hoverRef.current) return
      setQueue((q) =>
        q
          .map((tip, i) => (i < MAX_CARDS ? { ...tip, remainMs: tip.remainMs - TICK_MS } : tip))
          .filter((tip, i) => i >= MAX_CARDS || tip.remainMs > 0),
      )
    }, TICK_MS)
    return () => window.clearInterval(timer)
  }, [])

  const cards = queue.slice(0, MAX_CARDS)
  const overflowCount = Math.max(0, queue.length - MAX_CARDS)

  // 窗高同步（命中区=可见区）
  useEffect(() => {
    if (cards.length === 0 && toasts.length === 0) return
    const h =
      PAD * 2 +
      cards.length * CARD_H +
      (cards.length - 1) * GAP +
      (overflowCount > 0 ? GAP + MORE_H : 0) +
      toasts.length * (GAP + 36)
    void invoke('update_tips_height', { h }).catch(() => {})
  }, [cards.length, overflowCount])

  // 队列清空 → 隐藏窗口（tips 不落盘，退出即清空）
  useEffect(() => {
    if (queue.length === 0) void invoke('tips_dismissed').catch(() => {})
  }, [queue.length])

  const hasToast = toasts.length > 0
  if (cards.length === 0 && !hasToast) return null

  return (
    <div
      className="tips-root"
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      {cards.map((t) => (
        <TipCard
          key={t.id}
          id={t.id}
          kind={t.kind}
          name={t.name}
          from={t.from}
          sizeText={t.sizeText}
          remainMs={t.remainMs}
          totalMs={TOTAL_MS}
        />
      ))}
      {toasts.map((t) => (
        <div key={t.id} className="info warn" style={{ borderRadius: 8, flex: 'none' }}>
          {t.text}
        </div>
      ))}
      {overflowCount > 0 && (
        <div className="tip-more" onClick={() => void invoke('open_main')}>
          还有 {overflowCount} 条新共享 · 点击打开列表
        </div>
      )}
    </div>
  )
}
