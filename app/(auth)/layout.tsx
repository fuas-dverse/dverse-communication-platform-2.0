import { getCurrentUser } from '@/lib/auth'
import { redirect } from 'next/navigation'

export default async function AuthLayout({ children }: { children: React.ReactNode }) {
  const user = await getCurrentUser()
  if (user) redirect('/rooms')
  return <>{children}</>
}
