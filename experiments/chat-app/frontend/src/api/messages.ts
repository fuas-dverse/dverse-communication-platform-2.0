import { apiFetch } from "./client"
import type { Message } from "../types"

export async function getMessages(roomId: string): Promise<Message[]> {
  return apiFetch<Message[]>(`/rooms/${roomId}/messages`)
}

export async function sendMessage(
  roomId: string,
  content: string
): Promise<Message> {
  return apiFetch<Message>(`/rooms/${roomId}/messages`, {
    method: "POST",
    body: JSON.stringify({ content }),
  })
}
