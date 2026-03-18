import Anthropic from '@anthropic-ai/sdk'

const anthropic = new Anthropic()

interface MessageContext {
  username: string
  content: string
  is_bot: number
  created_at: number
}

function buildSystemPrompt(botName: string, history: MessageContext[]): string {
  const historyText = history
    .map(m => `[${m.is_bot ? botName : m.username}]: ${m.content}`)
    .join('\n')

  return `You are ${botName}, a helpful and friendly AI assistant in a chat room.
Users trigger you by mentioning @${botName} at the start of their message.
Be concise, helpful, and relevant to the conversation context.
Do not prefix your response with your name or any label.

Recent conversation history:
${historyText || '(no messages yet)'}`
}

// ── Claude (Anthropic API) ────────────────────────────────────────────────────

async function callClaude(
  botName: string,
  triggeringContent: string,
  history: MessageContext[]
): Promise<string> {
  const response = await anthropic.messages.create({
    model: 'claude-haiku-4-5-20251001',
    max_tokens: 1024,
    system: buildSystemPrompt(botName, history),
    messages: [{ role: 'user', content: triggeringContent }],
  })

  const block = response.content[0]
  if (block.type !== 'text') throw new Error('Unexpected response type from Claude')
  return block.text
}

// ── Local LLM via OpenAI-compatible API (Ollama, LM Studio, vLLM, etc.) ──────

async function callLocalLLM(
  botName: string,
  triggeringContent: string,
  history: MessageContext[]
): Promise<string> {
  const baseUrl = process.env.LOCAL_LLM_URL ?? 'http://localhost:11434'
  const model = process.env.LOCAL_LLM_MODEL ?? 'deepseek-r1:1.5b'

  const response = await fetch(`${baseUrl}/v1/chat/completions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      model,
      messages: [
        { role: 'system', content: buildSystemPrompt(botName, history) },
        { role: 'user', content: triggeringContent },
      ],
      stream: false,
    }),
  })

  if (!response.ok) {
    const text = await response.text()
    throw new Error(`Local LLM error ${response.status}: ${text}`)
  }

  const data = await response.json()
  const content: string = data.choices?.[0]?.message?.content
  if (!content) throw new Error('No content in local LLM response')

  // DeepSeek-R1 wraps its thinking in <think>…</think> tags — strip them
  return content.replace(/<think>[\s\S]*?<\/think>/g, '').trim()
}

// ── Public entry point ────────────────────────────────────────────────────────

export async function buildBotResponse(
  botName: string,
  triggeringContent: string,
  recentMessages: MessageContext[],
  provider: 'claude' | 'local' = 'claude'
): Promise<string> {
  if (provider === 'local') {
    return callLocalLLM(botName, triggeringContent, recentMessages)
  }
  return callClaude(botName, triggeringContent, recentMessages)
}
