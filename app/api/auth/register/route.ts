import { NextRequest, NextResponse } from 'next/server'
import bcrypt from 'bcryptjs'
import { stmts } from '@/lib/db'
import { signToken, setSessionCookie } from '@/lib/auth'

export async function POST(request: NextRequest) {
  const { username, password } = await request.json()

  if (
    !username ||
    typeof username !== 'string' ||
    username.length < 3 ||
    username.length > 20 ||
    !/^[a-zA-Z0-9_]+$/.test(username)
  ) {
    return NextResponse.json(
      { error: 'Username must be 3–20 alphanumeric characters or underscores' },
      { status: 400 }
    )
  }

  if (!password || typeof password !== 'string' || password.length < 8) {
    return NextResponse.json(
      { error: 'Password must be at least 8 characters' },
      { status: 400 }
    )
  }

  const existing = stmts.getUserByUsername.get(username)
  if (existing) {
    return NextResponse.json({ error: 'Username already taken' }, { status: 409 })
  }

  const passwordHash = await bcrypt.hash(password, 12)
  const id = crypto.randomUUID()
  const createdAt = Date.now()

  stmts.insertUser.run(id, username, passwordHash, createdAt)

  const token = await signToken({ userId: id, username })
  await setSessionCookie(token)

  return NextResponse.json({ user: { id, username } }, { status: 201 })
}
