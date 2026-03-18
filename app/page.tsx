import { getCurrentUser } from '@/lib/auth'
import { redirect } from 'next/navigation'

export default async function Home() {
  const user = await getCurrentUser()
  redirect(user ? '/rooms' : '/login')
}
