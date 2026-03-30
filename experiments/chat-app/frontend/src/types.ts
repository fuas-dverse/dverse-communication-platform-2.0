export type BotProvider = 'claude' | 'local' | 'zenoh'
export type BotPersonality = 'assistant' | 'coder' | 'creative' | 'analyst'

export interface BotConfig {
  id: string
  room_id: string
  name: string
  provider: BotProvider
  personality: BotPersonality
  model: string | null
  created_at: string
}

export interface BotConfigCreate {
  name: string
  provider: BotProvider
  personality: BotPersonality
  model?: string | null
}

export interface BotConfigUpdate {
  provider?: BotProvider
  personality?: BotPersonality
  model?: string | null
}

export interface Room {
  id: string
  name: string
  description: string
  server_id: string | null
  created_by: string
  created_at: string
  bots: BotConfig[]
}

export interface Message {
  id: string
  room_id: string
  user_id: string
  username: string
  content: string
  is_bot: boolean
  bot_id: string | null
  bot_triggered_by: string | null
  created_at: string
}

export interface User {
  id: string
  username: string
  created_at: string
}

export interface AuthResponse {
  access_token: string
  token_type: string
  user: User
}

export interface Server {
  id: string
  name: string
  description: string
  created_by: string
  created_at: string
  member_count: number
  invite_code: string | null
}

export interface ServerMember {
  user_id: string
  username: string
  joined_at: string
  is_online: boolean
  last_seen: string
}
