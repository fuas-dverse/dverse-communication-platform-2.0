import { apiFetch, API_BASE, getToken } from "./client"
import type { Server, ServerMember } from "../types"

export async function getServers(): Promise<Server[]> {
  return apiFetch<Server[]>("/servers")
}

export async function createServer(data: { name: string; description: string }): Promise<Server> {
  return apiFetch<Server>("/servers", {
    method: "POST",
    body: JSON.stringify(data),
  })
}

export async function joinServer(serverId: string): Promise<Server> {
  return apiFetch<Server>(`/servers/${serverId}/join`, { method: "POST" })
}

export async function joinServerByCode(code: string): Promise<Server> {
  return apiFetch<Server>(`/servers/join/${code}`, { method: "POST" })
}

export async function generateInviteCode(serverId: string): Promise<Server> {
  return apiFetch<Server>(`/servers/${serverId}/invite`, { method: "POST" })
}

export async function leaveServer(serverId: string): Promise<void> {
  await apiFetch<Record<string, never>>(`/servers/${serverId}/leave`, { method: "DELETE" })
}

export async function getServerMembers(serverId: string): Promise<ServerMember[]> {
  return apiFetch<ServerMember[]>(`/servers/${serverId}/members`)
}

export function streamServer(
  serverId: string,
  signal: AbortSignal,
): Promise<Response> {
  const token = getToken()
  return fetch(`${API_BASE}/servers/${serverId}/stream`, {
    headers: { Authorization: `Bearer ${token}` },
    signal,
  })
}
