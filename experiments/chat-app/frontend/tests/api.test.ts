import { beforeEach, describe, expect, it, vi } from "vitest"

import { API_BASE, apiFetch, clearToken, getToken, setToken } from "../src/api/client"
import { getMe, login, register } from "../src/api/auth"

type MockResponse = {
  ok: boolean
  statusText: string
  json: () => Promise<unknown>
}

const fetchMock = vi.fn()
const localStorageMock = {
  getItem: vi.fn(),
  setItem: vi.fn(),
  removeItem: vi.fn(),
}

beforeEach(() => {
  fetchMock.mockReset()
  localStorageMock.getItem.mockReset()
  localStorageMock.setItem.mockReset()
  localStorageMock.removeItem.mockReset()

  vi.stubGlobal("fetch", fetchMock)
  vi.stubGlobal("localStorage", localStorageMock)
})

describe("client storage helpers", () => {
  it("stores, reads, and clears the token", () => {
    localStorageMock.getItem.mockReturnValue("token-123")

    expect(getToken()).toBe("token-123")

    setToken("token-456")
    clearToken()

    expect(localStorageMock.setItem).toHaveBeenCalledWith("token", "token-456")
    expect(localStorageMock.removeItem).toHaveBeenCalledWith("token")
  })
})

describe("apiFetch", () => {
  it("adds the bearer token and custom headers", async () => {
    localStorageMock.getItem.mockReturnValue("token-abc")
    fetchMock.mockResolvedValue({
      ok: true,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({ ok: true }),
    } satisfies MockResponse)

    await apiFetch<{ ok: boolean }>("/rooms", {
      method: "POST",
      headers: { "X-Test": "yes" },
      body: JSON.stringify({ name: "general" }),
    })

    expect(fetchMock).toHaveBeenCalledWith(
      `${API_BASE}/rooms`,
      expect.objectContaining({
        method: "POST",
        headers: expect.objectContaining({
          "Content-Type": "application/json",
          Authorization: "Bearer token-abc",
          "X-Test": "yes",
        }),
      })
    )
  })

  it("throws the API error detail when the request fails", async () => {
    localStorageMock.getItem.mockReturnValue(null)
    fetchMock.mockResolvedValue({
      ok: false,
      statusText: "Bad Request",
      json: vi.fn().mockResolvedValue({ detail: "Invalid input" }),
    } satisfies MockResponse)

    await expect(apiFetch("/rooms")).rejects.toThrow("Invalid input")
  })
})

describe("auth helpers", () => {
  it("sends the correct login payload", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({
        access_token: "abc",
        token_type: "bearer",
        user: { id: "1", username: "alice", created_at: "now" },
      }),
    } satisfies MockResponse)

    await login("alice", "secret")

    expect(fetchMock).toHaveBeenCalledWith(
      `${API_BASE}/auth/login`,
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ username: "alice", password: "secret" }),
      })
    )
  })

  it("sends the correct register payload and getMe path", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      statusText: "OK",
      json: vi.fn().mockResolvedValue({
        access_token: "abc",
        token_type: "bearer",
        user: { id: "1", username: "alice", created_at: "now" },
      }),
    } satisfies MockResponse)

    await register("alice", "secret")
    await getMe()

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      `${API_BASE}/auth/register`,
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ username: "alice", password: "secret" }),
      })
    )
    expect(fetchMock).toHaveBeenNthCalledWith(2, `${API_BASE}/auth/me`, expect.anything())
  })
})
