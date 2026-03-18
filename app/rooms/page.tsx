import { getCurrentUser } from '@/lib/auth'
import { redirect } from 'next/navigation'
import { stmts } from '@/lib/db'
import RoomList from '@/components/RoomList'
import LogoutButton from '@/components/LogoutButton'

export default async function RoomsPage() {
  const user = await getCurrentUser()
  if (!user) redirect('/login')

  const rooms = stmts.getRooms.all()

  return (
    <div className="min-h-screen bg-gray-950">
      <header className="bg-gray-900 border-b border-gray-800 px-6 py-4 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-white font-bold text-lg">ChatApp</span>
        </div>
        <div className="flex items-center gap-4">
          <span className="text-gray-400 text-sm">@{user.username}</span>
          <LogoutButton />
        </div>
      </header>

      <main className="max-w-2xl mx-auto px-4 py-8">
        <RoomList initialRooms={rooms as Parameters<typeof RoomList>[0]['initialRooms']} currentUser={user} />
      </main>
    </div>
  )
}
