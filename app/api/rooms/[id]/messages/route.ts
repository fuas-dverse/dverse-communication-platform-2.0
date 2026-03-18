import { NextRequest, NextResponse } from 'next/server'
import { getCurrentUser } from '@/lib/auth'
import { stmts, BOT_USER_ID } from '@/lib/db'
import { buildBotResponse } from '@/lib/claude'
import { emitter, roomChannel } from '@/lib/sse'

export async function GET(
  request: NextRequest,
  { params }: { params: { id: string } }
) {
  const user = await getCurrentUser()
  if (!user) return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })

  const room = stmts.getRoomById.get(params.id)
  if (!room) return NextResponse.json({ error: 'Not found' }, { status: 404 })

  const after = request.nextUrl.searchParams.get('after')

  const messages = after
    ? stmts.getMessagesAfter.all(params.id, Number(after))
    : stmts.getMessages.all(params.id)

  return NextResponse.json({ messages })
}

function emitMessage(roomId: string, msgId: string) {
  const msg = stmts.getMessageById.get(msgId)
  if (msg) emitter.emit(roomChannel(roomId), msg)
}

export async function POST(
  request: NextRequest,
  { params }: { params: { id: string } }
) {
  const user = await getCurrentUser()
  if (!user) return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })

  const room = stmts.getRoomById.get(params.id) as {
    id: string
    has_bot: number
    bot_name: string
    bot_provider: string
  } | undefined
  if (!room) return NextResponse.json({ error: 'Not found' }, { status: 404 })

  const { content } = await request.json()
  if (!content || typeof content !== 'string' || content.trim().length === 0) {
    return NextResponse.json({ error: 'Content is required' }, { status: 400 })
  }

  const trimmed = content.trim().slice(0, 4000)
  const msgId = crypto.randomUUID()
  const created_at = Date.now()

  stmts.insertMessage.run(msgId, room.id, user.id, trimmed, 0, null, created_at)
  emitMessage(room.id, msgId)

  // Bot trigger check — fire and forget so the client gets 201 immediately
  if (room.has_bot === 1) {
    const trigger = `@${room.bot_name}`.toLowerCase()
    if (trimmed.toLowerCase().startsWith(trigger)) {
      const roomId = room.id
      const botName = room.bot_name
      const provider = room.bot_provider === 'local' ? 'local' : 'claude'
      const triggeredBy = user.username

      // Snapshot history now (synchronous) before returning the response
      const history = (stmts.getLastNMessages.all(roomId, 50) as {
        username: string
        content: string
        is_bot: number
        created_at: number
      }[]).reverse()

      // Emit a "thinking" placeholder so the UI shows the bot is working
      const thinkingId = crypto.randomUUID()
      stmts.insertMessage.run(thinkingId, roomId, BOT_USER_ID, '...', 1, triggeredBy, Date.now())
      emitMessage(roomId, thinkingId)

      // Process bot response asynchronously — don't block the HTTP response
      ;(async () => {
        try {
          const botReply = await buildBotResponse(botName, trimmed, history, provider)
          // Replace the placeholder with the real reply
          stmts.updateMessageContent.run(botReply, thinkingId)
          const updated = stmts.getMessageById.get(thinkingId)
          if (updated) emitter.emit(roomChannel(roomId), { ...updated, replace: true })
        } catch (e) {
          console.error('Bot response failed:', e)
          stmts.updateMessageContent.run('(bot failed to respond)', thinkingId)
          const updated = stmts.getMessageById.get(thinkingId)
          if (updated) emitter.emit(roomChannel(roomId), { ...updated, replace: true })
        }
      })()
    }
  }

  return NextResponse.json({ ok: true }, { status: 201 })
}
