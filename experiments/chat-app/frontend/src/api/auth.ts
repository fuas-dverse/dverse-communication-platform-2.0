import { apiFetch } from "./client"
import type { AuthResponse, User } from "../types"

export async function login(
  username: string,
  password: string
): Promise<AuthResponse> {
  return apiFetch<AuthResponse>("/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password }),
  })
}

export async function register(
  username: string,
  password: string
): Promise<AuthResponse> {
  return apiFetch<AuthResponse>("/auth/register", {
    method: "POST",
    body: JSON.stringify({ username, password }),
  })
}

export async function getMe(): Promise<User> {
  return apiFetch<User>("/auth/me")
}
