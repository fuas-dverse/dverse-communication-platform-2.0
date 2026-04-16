import {
  useState,
  useRef,
  useEffect,
  type KeyboardEvent,
  type ChangeEvent,
} from "react"
import { Icon } from "@iconify/react"

interface Props {
  onSend: (content: string) => Promise<void>
  disabled?: boolean
  placeholder?: string
  botNames?: string[]
}

export default function MessageInput({
  onSend,
  disabled,
  placeholder = "Type a message…",
  botNames = [],
}: Props) {
  const [value, setValue] = useState("")
  const [sending, setSending] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const isSending = sending || disabled
  const canSend = value.trim().length > 0 && !isSending

  function autoResize() {
    const el = textareaRef.current
    if (!el) return
    el.style.height = "auto"
    el.style.height = `${Math.min(el.scrollHeight, 120)}px`
  }

  useEffect(() => { autoResize() }, [value])

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

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  return (
    <div style={{ padding: "12px 16px", borderTop: "1px solid #2e3240", flexShrink: 0 }}>
      <div
        style={{
          background: "#1e2229",
          borderRadius: "10px",
          border: "1px solid #2e3240",
          display: "flex",
          alignItems: "flex-end",
          gap: "8px",
          padding: "8px 10px",
        }}
        onFocus={(e) => (e.currentTarget.style.borderColor = "#4a4f6a")}
        onBlur={(e) => (e.currentTarget.style.borderColor = "#2e3240")}
      >
        {/* Left buttons */}
        <div style={{ display: "flex", gap: "4px", alignItems: "center" }}>
          <InpBtn title="Attach file"><Icon icon="lucide:paperclip" style={{ fontSize: "15px" }} /></InpBtn>
          <InpBtn title="Emoji"><Icon icon="lucide:smile" style={{ fontSize: "15px" }} /></InpBtn>
        </div>

        {/* Textarea */}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e: ChangeEvent<HTMLTextAreaElement>) => setValue(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={!!isSending}
          rows={1}
          placeholder={placeholder}
          style={{
            flex: 1,
            background: "transparent",
            border: "none",
            outline: "none",
            color: "#e0e2ea",
            fontSize: "13px",
            resize: "none",
            lineHeight: "1.5",
            minHeight: "20px",
            maxHeight: "120px",
            fontFamily: "inherit",
          }}
        />

        {/* Right buttons */}
        <div style={{ display: "flex", gap: "4px", alignItems: "center" }}>
          <InpBtn title="Format text"><Icon icon="lucide:type" style={{ fontSize: "14px" }} /></InpBtn>
          <button
            onClick={handleSend}
            disabled={!canSend}
            title="Send message"
            style={{
              width: "28px",
              height: "28px",
              borderRadius: "7px",
              background: canSend ? "#5865f2" : "#2e3345",
              border: "none",
              color: canSend ? "#fff" : "#5f6478",
              cursor: canSend ? "pointer" : "default",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              transition: "background 0.1s",
              flexShrink: 0,
            }}
          >
            <Icon icon="lucide:send-horizontal" style={{ fontSize: "14px" }} />
          </button>
        </div>
      </div>

      {/* Bot hint */}
      {botNames.length > 0 && (
        <div style={{ fontSize: "11px", color: "#5f6478", marginTop: "5px", padding: "0 2px" }}>
          Tip: Start with{" "}
          {botNames.map((name, i) => (
            <span key={name}>
              <span style={{ color: "#57f2b8", fontWeight: "500" }}>@{name}</span>
              {i < botNames.length - 1 ? ", " : ""}
            </span>
          ))}{" "}
          to invoke an AI agent
        </div>
      )}
    </div>
  )
}

function InpBtn({ children, title }: { children: React.ReactNode; title: string }) {
  return (
    <button
      title={title}
      style={{
        width: "26px", height: "26px", borderRadius: "6px",
        border: "none", background: "transparent",
        color: "#5f6478", cursor: "pointer",
        display: "flex", alignItems: "center", justifyContent: "center",
        flexShrink: 0,
      }}
      onMouseEnter={(e) => { e.currentTarget.style.background = "#2e3345"; e.currentTarget.style.color = "#9a9fad" }}
      onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.style.color = "#5f6478" }}
    >
      {children}
    </button>
  )
}
