import type { Metadata } from 'next'
import './globals.css'

export const metadata: Metadata = {
  title: 'ChatApp',
  description: 'Real-time chat with AI bots',
}

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="bg-gray-950 text-white antialiased">
        {children}
      </body>
    </html>
  )
}
