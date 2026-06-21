import { apiFetch } from "./client"

export interface ZenohSessionInfo {
  connected: boolean
  namespace: string
}

export interface DverseNode {
  cn: string
  name: string
  agents: string[]
  online: boolean
  last_seen: number
}

export async function getZenohSession(): Promise<ZenohSessionInfo> {
  return apiFetch<ZenohSessionInfo>("/zenoh/session")
}

export async function connectZenohSession(
  opts: { router?: string; namespace?: string; connection_string?: string },
): Promise<ZenohSessionInfo> {
  return apiFetch<ZenohSessionInfo>("/zenoh/session", {
    method: "PUT",
    body: JSON.stringify(opts),
  })
}

export async function disconnectZenohSession(): Promise<void> {
  await apiFetch<unknown>("/zenoh/session", { method: "DELETE" })
}

export async function getZenohNodes(): Promise<DverseNode[]> {
  return apiFetch<DverseNode[]>("/zenoh/session/nodes")
}
