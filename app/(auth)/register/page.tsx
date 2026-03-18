'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import Link from 'next/link'

export default function RegisterPage() {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const router = useRouter()

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setLoading(true)
    setError(null)

    const res = await fetch('/api/auth/register', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, password }),
    })

    const data = await res.json()
    if (!res.ok) {
      setError(data.error ?? 'Registration failed')
      setLoading(false)
      return
    }

    router.push('/rooms')
  }

  return (
    <div className="min-h-screen bg-gray-950 flex items-center justify-center px-4">
      <div className="bg-gray-900 p-8 rounded-2xl w-full max-w-sm border border-gray-800">
        <div className="mb-8">
          <h1 className="text-white text-2xl font-bold">Create account</h1>
          <p className="text-gray-400 text-sm mt-1">Join the conversation</p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          {error && (
            <p className="text-red-400 text-sm bg-red-950/50 border border-red-900/50 px-3 py-2 rounded-lg">
              {error}
            </p>
          )}

          <div>
            <label className="text-gray-400 text-sm block mb-1.5">Username</label>
            <input
              type="text"
              value={username}
              onChange={e => setUsername(e.target.value)}
              placeholder="3–20 characters, letters/numbers/_"
              className="w-full bg-gray-800 text-white px-4 py-2.5 rounded-lg border border-gray-700 focus:outline-none focus:border-indigo-500 transition-colors placeholder:text-gray-600"
              required
              autoFocus
            />
          </div>

          <div>
            <label className="text-gray-400 text-sm block mb-1.5">Password</label>
            <input
              type="password"
              value={password}
              onChange={e => setPassword(e.target.value)}
              placeholder="At least 8 characters"
              className="w-full bg-gray-800 text-white px-4 py-2.5 rounded-lg border border-gray-700 focus:outline-none focus:border-indigo-500 transition-colors placeholder:text-gray-600"
              required
            />
          </div>

          <button
            type="submit"
            disabled={loading}
            className="w-full bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold py-2.5 rounded-lg transition-colors mt-2"
          >
            {loading ? 'Creating account…' : 'Create Account'}
          </button>
        </form>

        <p className="text-gray-500 text-sm mt-6 text-center">
          Have an account?{' '}
          <Link href="/login" className="text-indigo-400 hover:text-indigo-300 transition-colors">
            Sign in
          </Link>
        </p>
      </div>
    </div>
  )
}
