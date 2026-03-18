import { NextRequest, NextResponse } from 'next/server'
import bcrypt from 'bcryptjs'
import { stmts } from '@/lib/db'
import { signToken, setSessionCookie } from '@/lib/auth'

export async function POST(request: NextRequest) {
  const { username, password } = await request.json()

  if (!username || !password) {
    return NextResponse.json({ error: 'Username and password are required' }, { status: 400 })
  }

  const user = stmts.getUserByUsername.get(username) as
    | { id: string; username: string; password_hash: string }
    | undefined

  const INVALID = 'Invalid username or password'

  if (!user) {
    // Constant-time response to prevent username enumeration
    await bcrypt.hash('dummy', 12)
    return NextResponse.json({ error: INVALID }, { status: 401 })
  }

  const match = await bcrypt.compare(password, user.password_hash)
  if (!match) {
    return NextResponse.json({ error: INVALID }, { status: 401 })
  }

  const token = await signToken({ userId: user.id, username: user.username })
  await setSessionCookie(token)

  return NextResponse.json({ user: { id: user.id, username: user.username } })
}
