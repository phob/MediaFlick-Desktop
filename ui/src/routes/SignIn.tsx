import { Check, Copy, Settings2 } from "lucide-react"
import { useEffect, useState } from "react"
import { Link } from "react-router-dom"
import { toast } from "sonner"
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
import { looksLikeServerAddress } from "@/lib/server-address"

const PROBE_DEBOUNCE_MS = 400
const COPIED_FEEDBACK_MS = 2000

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
  const serverAddress = serverValue.trim()
  const addressComplete = looksLikeServerAddress(serverAddress)
  // The last address the user settled on, by pausing on a complete-looking
  // address or by leaving the field. It probes only while the field still holds it.
  const [settled, setSettled] = useState("")
  const probeTarget = server === null
    ? settings.data?.serverUrl ?? ""
    : settled === serverAddress ? settled : ""
  const info = useServerInfo(probeTarget)

  useEffect(() => {
    if (!addressComplete) return
    const timer = window.setTimeout(() => setSettled(serverAddress), PROBE_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [addressComplete, serverAddress])

  const quickConnect = useQuickConnectStart()
  const poll = useQuickConnectPoll(quickConnect.data)

  const quickError = quickConnect.error ?? poll.error
  const quickConnectAvailable = Boolean(serverAddress && info.data?.quickConnect)
  let quickConnectHelp = "Enter your server address first to use Quick Connect without a password."
  if (serverAddress) {
    if (!probeTarget && !addressComplete) {
      quickConnectHelp = "Finish the server address to check Quick Connect availability."
    } else if (!probeTarget || info.isPending) {
      quickConnectHelp = "Checking Quick Connect availability…"
    } else if (info.error) {
      quickConnectHelp = "Could not check Quick Connect. Check your server address and connection."
    } else if (!info.data?.quickConnect) {
      quickConnectHelp = "Quick Connect is not enabled on this server. Sign in with your username and password."
    } else {
      quickConnectHelp = "Sign in by approving a code on a device already signed in to this Jellyfin server."
    }
  }

  return (
    <div className="signin-page relative grid h-full place-items-center overflow-x-hidden overflow-y-auto p-6">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_18%_18%,rgba(229,9,20,0.16),transparent_32rem),radial-gradient(circle_at_85%_80%,rgba(255,255,255,0.05),transparent_30rem)]" />
      <div className="relative z-10 flex w-full max-w-md flex-col gap-7">
        <img
          src={appIcon}
          alt="MediaFlick"
          className="mx-auto size-28 drop-shadow-[0_1.25rem_2rem_rgb(0_0_0/45%)]"
        />

      <Card className="cinematic-panel w-full gap-5 rounded-2xl py-7">
        <CardHeader className="gap-2">
          <CardTitle className="text-xl">Welcome to MediaFlick</CardTitle>
          <CardDescription>Connect to your Jellyfin server to continue.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault()
              login.mutate({ server: serverAddress, username, password })
            }}
          >
            <div className="space-y-1.5">
              <Label htmlFor="server">Server</Label>
              <Input
                id="server"
                value={serverValue}
                onChange={(event) => {
                  setServer(event.target.value)
                  if (event.target.value.trim() === serverAddress) return
                  // Stop displaying and polling a code as soon as its server changes.
                  quickConnect.reset()
                }}
                // An address the completeness check does not recognize, such as a
                // single-label LAN host, is still probed once the user moves on.
                onBlur={() => setSettled(serverAddress)}
                placeholder="https://jellyfin.example.com"
                autoComplete="url"
                className="h-11 border-edge bg-field"
              />
            </div>
            <div className="flex flex-col gap-3 border-b border-white/8 pb-4">
              <p id="quick-connect-help" className="text-sm text-muted-foreground" role="status">
                {quickConnectHelp}
              </p>
              {quickConnect.data ? (
                <div className="flex flex-col items-center gap-1">
                  <p className="text-sm text-muted-foreground">In Jellyfin, open your user menu → Quick Connect and enter:</p>
                  <div className="relative">
                    <p className="text-2xl font-medium tracking-[0.3em]">{quickConnect.data.code}</p>
                    <CopyCodeButton code={quickConnect.data.code} />
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {poll.data?.authenticated ? "Approved — signing in…" : "Waiting for approval…"}
                  </p>
                </div>
              ) : (
                <Button
                  type="button"
                  variant="secondary"
                  aria-describedby="quick-connect-help"
                  disabled={quickConnect.isPending || !quickConnectAvailable}
                  onClick={() => quickConnect.mutate(serverAddress)}
                >
                  {quickConnect.isPending ? "Starting…" : "Use Quick Connect"}
                </Button>
              )}
              {quickError && <p className="text-sm text-destructive">{quickError.message}</p>}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                value={username}
                onChange={(event) => setUsername(event.target.value)}
                placeholder="Your Jellyfin username"
                autoComplete="username"
                className="h-11 border-edge bg-field"
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
                className="h-11 border-edge bg-field"
              />
            </div>
            {login.error && <p className="text-sm text-destructive">{login.error.message}</p>}
            <Button type="submit" size="lg" disabled={login.isPending || !serverAddress}>
              {login.isPending ? "Signing in…" : "Sign in"}
            </Button>
          </form>

          {info.data && (
            <p className="text-center text-xs text-muted-foreground">
              {[info.data.serverName ?? "Jellyfin", info.data.version].filter(Boolean).join(" · ")}
            </p>
          )}
        </CardContent>
      </Card>
      <Button asChild variant="link" className="mx-auto text-muted-foreground hover:text-foreground">
        <Link to="/settings/client/player">
          <Settings2 />
          Player and app settings
        </Link>
      </Button>
      </div>
    </div>
  )
}

// The page disables text selection, so the code needs its own way to the clipboard.
function CopyCodeButton({ code }: { code: string }) {
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!copied) return
    const timer = window.setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS)
    return () => window.clearTimeout(timer)
  }, [copied])

  const Icon = copied ? Check : Copy
  return (
    <Button
      type="button"
      size="icon-sm"
      variant="ghost"
      aria-label={copied ? "Copied" : "Copy code"}
      className="absolute top-1/2 left-full ml-1 -translate-y-1/2 text-muted-foreground hover:text-foreground"
      onClick={() => {
        navigator.clipboard.writeText(code).then(
          () => setCopied(true),
          () => toast.error("Could not copy the code"),
        )
      }}
    >
      <Icon />
    </Button>
  )
}
