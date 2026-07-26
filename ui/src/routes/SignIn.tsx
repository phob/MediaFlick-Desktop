import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import {
  useLogin,
  useQuickConnectPoll,
  useQuickConnectStart,
  useServerInfo,
  useSettings,
} from "@/lib/queries"

export default function SignIn() {
  const settings = useSettings()
  const login = useLogin()
  // `null` is "untouched", which is what lets the saved URL prefill the field
  // once settings arrive. An empty string is a field the user cleared on
  // purpose, and has to stay cleared so it can be retyped from scratch.
  const [server, setServer] = useState<string | null>(null)
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")

  const serverValue = server ?? settings.data?.serverUrl ?? ""
  // Probing only on blur keeps a half-typed address from being dialled on
  // every keystroke.
  const [probed, setProbed] = useState<string | null>(null)
  const info = useServerInfo(probed ?? settings.data?.serverUrl ?? "")

  const quickConnect = useQuickConnectStart()
  const poll = useQuickConnectPoll(quickConnect.data)

  const quickError = quickConnect.error ?? poll.error

  return (
    <div className="grid h-full place-items-center p-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>Sign in to Jellyfin</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault()
              login.mutate({ server: serverValue, username, password })
            }}
          >
            <Input
              value={serverValue}
              onChange={(event) => setServer(event.target.value)}
              onBlur={(event) => {
                const next = event.target.value.trim()
                if (next === probed) return
                setProbed(next)
                // A code issued by the previous server is worthless now.
                quickConnect.reset()
              }}
              placeholder="https://jellyfin.example.com"
              autoComplete="url"
            />
            <Input
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              placeholder="Username"
              autoComplete="username"
            />
            <Input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder="Password"
              autoComplete="current-password"
            />
            {login.error && <p className="text-sm text-destructive">{login.error.message}</p>}
            <Button type="submit" disabled={login.isPending || !serverValue}>
              {login.isPending ? "Signing in…" : "Sign in"}
            </Button>
          </form>

          {info.data && (
            <p className="text-center text-xs text-muted-foreground">
              {[info.data.serverName ?? "Jellyfin", info.data.version].filter(Boolean).join(" · ")}
            </p>
          )}

          {/* Only offered where the server actually has it enabled — an
              always-visible button that answers 501 is worse than no button. */}
          {info.data?.quickConnect && (
            <div className="flex flex-col gap-2 border-t pt-4">
              {quickConnect.data ? (
                <div className="flex flex-col items-center gap-1">
                  <p className="text-sm text-muted-foreground">Enter this code in Jellyfin</p>
                  <p className="text-2xl font-medium tracking-[0.3em]">{quickConnect.data.code}</p>
                  <p className="text-xs text-muted-foreground">
                    {poll.data?.authenticated ? "Approved — signing in…" : "Waiting for approval…"}
                  </p>
                </div>
              ) : (
                <Button
                  variant="secondary"
                  disabled={quickConnect.isPending || !serverValue}
                  onClick={() => quickConnect.mutate(serverValue)}
                >
                  {quickConnect.isPending ? "Starting…" : "Use Quick Connect"}
                </Button>
              )}
              {quickError && <p className="text-sm text-destructive">{quickError.message}</p>}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
