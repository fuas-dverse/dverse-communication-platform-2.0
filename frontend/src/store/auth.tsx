import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  type ReactNode,
} from "react"
import { getToken, setToken, clearToken } from "../api/client"
import {
  login as apiLogin,
  register as apiRegister,
  getMe,
} from "../api/auth"
import type { User } from "../types"

interface AuthContextType {
  user: User | null
  token: string | null
  login: (username: string, password: string) => Promise<void>
  register: (username: string, password: string) => Promise<void>
  logout: () => void
  loading: boolean
}

const AuthContext = createContext<AuthContextType | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [token, setTokenState] = useState<string | null>(getToken())
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const existingToken = getToken()
    if (!existingToken) {
      setLoading(false)
      return
    }

    getMe()
      .then((me) => {
        setUser(me)
        setTokenState(existingToken)
      })
      .catch(() => {
        clearToken()
        setTokenState(null)
      })
      .finally(() => {
        setLoading(false)
      })
  }, [])

  const login = useCallback(async (username: string, password: string) => {
    const res = await apiLogin(username, password)
    setToken(res.access_token)
    setTokenState(res.access_token)
    setUser(res.user)
  }, [])

  const register = useCallback(async (username: string, password: string) => {
    const res = await apiRegister(username, password)
    setToken(res.access_token)
    setTokenState(res.access_token)
    setUser(res.user)
  }, [])

  const logout = useCallback(() => {
    clearToken()
    setTokenState(null)
    setUser(null)
  }, [])

  return (
    <AuthContext.Provider value={{ user, token, login, register, logout, loading }}>
      {children}
    </AuthContext.Provider>
  )
}

export function useAuth(): AuthContextType {
  const ctx = useContext(AuthContext)
  if (!ctx) {
    throw new Error("useAuth must be used within AuthProvider")
  }
  return ctx
}
