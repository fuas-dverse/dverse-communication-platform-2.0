'use client'

import { useRouter } from 'next/navigation'

export default function LogoutButton() {
  const router = useRouter()

  async function logout() {
    await fetch('/api/auth/logout', { method: 'POST' })
    router.push('/login')
  }

  return (
    <button
      onClick={logout}
      className="text-gray-400 hover:text-white text-sm transition-colors"
    >
      Logout
    </button>
  )
}
