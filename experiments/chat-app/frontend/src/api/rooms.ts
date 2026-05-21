import { apiFetch } from "./client"
import type { Room, BotConfig, BotConfigCreate, BotConfigUpdate, AvailableBot } from "../types"

export async function getRooms(serverId?: string): Promise<Room[]> {
  const url = serverId ? `/rooms?server_id=${encodeURIComponent(serverId)}` : "/rooms"
  return apiFetch<Room[]>(url)
}

export async function createRoom(data: {
  name: string
  description: string
  server_id?: string | null
}): Promise<Room> {
  return apiFetch<Room>("/rooms", {
    method: "POST",
    body: JSON.stringify(data),
  })
}

export async function getRoom(id: string): Promise<Room> {
  return apiFetch<Room>(`/rooms/${id}`)
}

export async function addBot(
  roomId: string,
  data: BotConfigCreate
): Promise<BotConfig> {
  return apiFetch<BotConfig>(`/rooms/${roomId}/bots`, {
    method: "POST",
    body: JSON.stringify(data),
  })
}

export async function updateBot(
  roomId: string,
  botId: string,
  data: BotConfigUpdate
): Promise<BotConfig> {
  return apiFetch<BotConfig>(`/rooms/${roomId}/bots/${botId}`, {
    method: "PATCH",
    body: JSON.stringify(data),
  })
}

export async function deleteBot(
  roomId: string,
  botId: string
): Promise<void> {
  await apiFetch<Record<string, never>>(`/rooms/${roomId}/bots/${botId}`, {
    method: "DELETE",
  })
}

export async function getAvailableBots(): Promise<AvailableBot[]> {
  return apiFetch<AvailableBot[]>("/bots/available")
}
