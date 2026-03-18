import { NextRequest, NextResponse } from 'next/server'
import { getCurrentUser } from '@/lib/auth'
import { stmts } from '@/lib/db'

export async function GET(
  _request: NextRequest,
  { params }: { params: { id: string } }
) {
  const room = stmts.getRoomById.get(params.id)
  if (!room) return NextResponse.json({ error: 'Not found' }, { status: 404 })
  return NextResponse.json({ room })
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: { id: string } }
) {
  const user = await getCurrentUser()
  if (!user) return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })

  const room = stmts.getRoomById.get(params.id) as { created_by: string } | undefined
  if (!room) return NextResponse.json({ error: 'Not found' }, { status: 404 })
  if (room.created_by !== user.id) {
    return NextResponse.json({ error: 'Only the room creator can change bot settings' }, { status: 403 })
  }

  const { has_bot, bot_name, bot_provider } = await request.json()
  const safeBotName = (typeof bot_name === 'string' && bot_name.trim()) ? bot_name.trim() : 'bot'
  const safeProvider = bot_provider === 'local' ? 'local' : 'claude'

  stmts.updateRoomBot.run(has_bot ? 1 : 0, safeBotName, safeProvider, params.id)

  return NextResponse.json({ ok: true })
}
