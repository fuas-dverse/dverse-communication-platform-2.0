import {
  useState,
  useRef,
  useEffect,
  type KeyboardEvent,
  type ChangeEvent,
} from "react"

interface Props {
  onSend: (content: string) => Promise<void>
  disabled?: boolean
}

export default function MessageInput({ onSend, disabled }: Props) {
  const [value, setValue] = useState("")
  const [sending, setSending] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const isSending = sending || disabled
  const showBotHint = value.startsWith("@")

  function autoResize() {
    const el = textareaRef.current
    if (!el) return
    el.style.height = "auto"
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`
  }

  useEffect(() => {
    autoResize()
  }, [value])

  async function handleSend() {
    const trimmed = value.trim()
    if (!trimmed || isSending) return

    setSending(true)
    try {
      await onSend(trimmed)
      setValue("")
    } finally {
      setSending(false)
      textareaRef.current?.focus()
    }
  }

  function handleChange(e: ChangeEvent<HTMLTextAreaElement>) {
    setValue(e.target.value)
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  return (
    <div className="border-t border-gray-700 bg-gray-900 px-4 py-3">
      {showBotHint && (
        <div className="mb-2 text-xs text-indigo-400 flex items-center gap-1.5">
          <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          Bot will respond to your message
        </div>
      )}
      <div className="flex items-end gap-3">
        <textarea
          ref={textareaRef}
          value={value}
          onChange={handleChange}
          onKeyDown={handleKeyDown}
          disabled={isSending}
          rows={1}
          placeholder="Type a message... (Shift+Enter for newline)"
          className="flex-1 bg-gray-800 border border-gray-700 focus:border-indigo-500 text-white rounded-xl px-4 py-3 text-sm resize-none focus:outline-none focus:ring-1 focus:ring-indigo-500 placeholder-gray-500 transition scrollbar-thin disabled:opacity-60 disabled:cursor-not-allowed"
        />
        <button
          onClick={handleSend}
          disabled={!value.trim() || isSending}
          className="flex-shrink-0 bg-indigo-600 hover:bg-indigo-500 disabled:bg-gray-700 disabled:cursor-not-allowed text-white rounded-xl px-4 py-3 transition flex items-center justify-center"
          aria-label="Send message"
        >
          {sending ? (
            <svg className="w-5 h-5 animate-spin" fill="none" viewBox="0 0 24 24">
              <circle
                className="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                strokeWidth="4"
              />
              <path
                className="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
              />
            </svg>
          ) : (
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8"
              />
            </svg>
          )}
        </button>
      </div>
    </div>
  )
}
