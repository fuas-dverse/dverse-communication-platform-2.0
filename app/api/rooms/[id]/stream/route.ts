import { NextRequest } from 'next/server'
import { getCurrentUser } from '@/lib/auth'
import { stmts } from '@/lib/db'
import { emitter, roomChannel } from '@/lib/sse'

export async function GET(
  request: NextRequest,
  { params }: { params: { id: string } }
) {
  const user = await getCurrentUser()
  if (!user) return new Response('Unauthorized', { status: 401 })

  const room = stmts.getRoomById.get(params.id)
  if (!room) return new Response('Not found', { status: 404 })

  const channel = roomChannel(params.id)
  const encoder = new TextEncoder()

  const stream = new ReadableStream({
    start(controller) {
      // Send a ping immediately so the client knows the connection is alive
      controller.enqueue(encoder.encode('data: ping\n\n'))

      const handler = (message: unknown) => {
        try {
          controller.enqueue(encoder.encode(`data: ${JSON.stringify(message)}\n\n`))
        } catch {
          // Controller already closed
        }
      }

      emitter.on(channel, handler)

      // Clean up when client disconnects
      request.signal.addEventListener('abort', () => {
        emitter.off(channel, handler)
        try { controller.close() } catch { /* already closed */ }
      })
    },
  })

  return new Response(stream, {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache, no-transform',
      'Connection': 'keep-alive',
      'X-Accel-Buffering': 'no', // Disable nginx buffering if behind a proxy
    },
  })
}
