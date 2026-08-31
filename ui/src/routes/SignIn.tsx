import { useState } from "react"
import appIcon from "../../../distribution/app-icon.png?inline"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
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
    <div className="signin-page relative grid h-full place-items-center overflow-hidden p-6">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_18%_18%,rgba(229,9,20,0.16),transparent_32rem),radial-gradient(circle_at_85%_80%,rgba(255,255,255,0.05),transparent_30rem)]" />
      <div className="relative z-10 flex w-full max-w-md flex-col gap-7">
        <img
          src={appIcon}
          alt="MediaFlick"
          className="mx-auto size-28 drop-shadow-[0_1.25rem_2rem_rgb(0_0_0/45%)]"
        />

      <Card className="cinematic-panel w-full gap-5 rounded-2xl py-7">
        <CardHeader className="gap-2">
          <CardTitle className="text-xl">Welcome back</CardTitle>
          <CardDescription>Connect to your Jellyfin server to continue.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault()
              login.mutate({ server: serverValue, username, password })
            }}
          >
            <div className="space-y-1.5">
              <Label htmlFor="server">Server</Label>
              <Input
                id="server"
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
                className="h-11 border-white/10 bg-white/5"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                value={username}
                onChange={(event) => setUsername(event.target.value)}
                placeholder="Your Jellyfin username"
                autoComplete="username"
                className="h-11 border-white/10 bg-white/5"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                placeholder="Your Jellyfin password"
                autoComplete="current-password"
                className="h-11 border-white/10 bg-white/5"
              />
            </div>
            {login.error && <p className="text-sm text-destructive">{login.error.message}</p>}
            <Button type="submit" size="lg" disabled={login.isPending || !serverValue}>
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
            <div className="flex flex-col gap-3 border-t border-white/8 pt-5">
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
    </div>
  )
}
