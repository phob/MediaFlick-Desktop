import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { useLogin, useSettings } from "@/lib/queries"

export default function SignIn() {
  const settings = useSettings()
  const login = useLogin()
  const [server, setServer] = useState("")
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")

  // The saved server URL prefills the field once settings arrive.
  const serverValue = server || settings.data?.serverUrl || ""

  return (
    <div className="grid h-full place-items-center p-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>Sign in to Jellyfin</CardTitle>
        </CardHeader>
        <CardContent>
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
          {/* TODO(port): Quick Connect, via api.quickConnectStart/quickConnectPoll. */}
        </CardContent>
      </Card>
    </div>
  )
}
