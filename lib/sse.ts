import { EventEmitter } from 'events'

declare global {
  // eslint-disable-next-line no-var
  var __emitter: EventEmitter | undefined
}

// Global singleton so all route handlers share the same emitter
export const emitter: EventEmitter =
  globalThis.__emitter ?? (globalThis.__emitter = new EventEmitter())

// Allow many concurrent SSE connections per room
emitter.setMaxListeners(200)

if (process.env.NODE_ENV === 'development') {
  globalThis.__emitter = emitter
}

export function roomChannel(roomId: string) {
  return `room:${roomId}`
}
