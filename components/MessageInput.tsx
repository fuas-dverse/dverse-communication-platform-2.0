'use client'

import { useState, useRef, KeyboardEvent } from 'react'

interface Props {
  onSend: (content: string) => Promise<void>
  isSending: boolean
  hasBotHint: boolean
  botName: string
}

export default function MessageInput({ onSend, isSending, hasBotHint, botName }: Props) {
  const [value, setValue] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const isTriggeringBot =
    hasBotHint && value.toLowerCase().startsWith(`@${botName.toLowerCase()}`)

  async function handleSend() {
    const trimmed = value.trim()
    if (!trimmed || isSending) return
    setValue('')
    // Reset textarea height
    if (textareaRef.current) textareaRef.current.style.height = 'auto'
    await onSend(trimmed)
    textareaRef.current?.focus()
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  function handleInput(e: React.ChangeEvent<HTMLTextAreaElement>) {
    setValue(e.target.value)
    // Auto-resize
    const ta = e.target
    ta.style.height = 'auto'
    ta.style.height = Math.min(ta.scrollHeight, 120) + 'px'
  }

  return (
    <div className="bg-gray-900 border-t border-gray-800 p-4 shrink-0">
      {isTriggeringBot && (
        <div className="mb-2 flex items-center gap-1.5">
          <span className="text-xs bg-indigo-950 text-indigo-400 border border-indigo-800/50 px-2.5 py-1 rounded-full">
            AI will respond to this message
          </span>
        </div>
      )}

      <div className="flex gap-3 items-end">
        <textarea
          ref={textareaRef}
          value={value}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          placeholder={
            hasBotHint
              ? `Message… (type @${botName} to ask the AI)`
              : 'Message… (Enter to send, Shift+Enter for newline)'
          }
          rows={1}
          className="flex-1 bg-gray-800 text-white px-4 py-2.5 rounded-xl border border-gray-700 focus:outline-none focus:border-indigo-500 resize-none text-sm transition-colors placeholder:text-gray-600 leading-relaxed"
          style={{ minHeight: '44px', maxHeight: '120px' }}
        />
        <button
          onClick={handleSend}
          disabled={!value.trim() || isSending}
          className="bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 disabled:cursor-not-allowed text-white px-4 py-2.5 rounded-xl font-medium text-sm transition-colors shrink-0 h-11"
        >
          {isSending ? (
            <span className="flex items-center gap-1.5">
              <span className="inline-block w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin" />
            </span>
          ) : (
            'Send'
          )}
        </button>
      </div>
    </div>
  )
}
