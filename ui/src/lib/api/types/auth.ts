export interface ServerInfo {
  serverUrl: string
  serverName: string | null
  version: string | null
  quickConnect: boolean
}

export interface QuickConnectStart {
  serverUrl: string
  code: string
  secret: string
}
