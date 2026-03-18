import { NextRequest, NextResponse } from 'next/server'
import { getCurrentUser } from '@/lib/auth'
import { stmts } from '@/lib/db'

export async function GET() {
  const rooms = stmts.getRooms.all()
  return NextResponse.json({ rooms })
}

export async function POST(request: NextRequest) {
  const user = await getCurrentUser()
  if (!user) return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })

  const { name, description = '', has_bot = false, bot_name = 'bot', bot_provider = 'claude' } = await request.json()

  if (!name || typeof name !== 'string' || name.trim().length === 0 || name.length > 50) {
    return NextResponse.json({ error: 'Room name must be 1–50 characters' }, { status: 400 })
  }

  const id = crypto.randomUUID()
  const created_at = Date.now()
  const trimmedName = name.trim()
  const safeBotName = (typeof bot_name === 'string' && bot_name.trim()) ? bot_name.trim() : 'bot'
  const safeProvider = bot_provider === 'local' ? 'local' : 'claude'

  try {
    stmts.insertRoom.run(id, trimmedName, description, user.id, has_bot ? 1 : 0, safeBotName, safeProvider, created_at)
  } catch (e: unknown) {
    if (e instanceof Error && e.message.includes('UNIQUE constraint')) {
      return NextResponse.json({ error: 'Room name already taken' }, { status: 409 })
    }
    throw e
  }

  return NextResponse.json(
    { room: { id, name: trimmedName, description, created_by: user.id, has_bot: has_bot ? 1 : 0, bot_name: safeBotName, bot_provider: safeProvider, created_at } },
    { status: 201 }
  )
}
