import { getCurrentUser } from '@/lib/auth'
import { stmts } from '@/lib/db'
import { redirect, notFound } from 'next/navigation'
import ChatRoom from '@/components/ChatRoom'
import type { Message } from '@/components/MessageList'

export default async function RoomPage({
  params,
}: {
  params: { id: string }
}) {
  const user = await getCurrentUser()
  if (!user) redirect('/login')

  const room = stmts.getRoomById.get(params.id) as {
    id: string
    name: string
    description: string
    created_by: string
    has_bot: number
    bot_name: string
    bot_provider: string
  } | undefined

  if (!room) notFound()

  const initialMessages = stmts.getMessages.all(params.id) as Message[]

  return (
    <ChatRoom
      room={room}
      currentUser={user}
      initialMessages={initialMessages}
    />
  )
}
