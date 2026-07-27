import { useState } from "react"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  useSeerrConnect,
  useSeerrLink,
  useSeerrLinkPassword,
  useSeerrStatus,
  useSeerrUnlink,
} from "@/lib/queries"

/**
 * Seerr setup, as a React dialog off the user menu rather than a page in the
 * native Client Settings: there is no `POST /api/settings` to hang it on, and
 * the flow is interactive — probe, then link, with a password step that only
 * appears when the password-less path is unavailable.
 */
export function SeerrSetupDialog({ onClose }: { onClose: () => void }) {
  const status = useSeerrStatus()
  const connect = useSeerrConnect()
  const link = useSeerrLink()
  const linkPassword = useSeerrLinkPassword()
  const unlink = useSeerrUnlink()

  // `null` is "untouched", so the stored address prefills the field but a
  // cleared one stays cleared.
  const [server, setServer] = useState<string | null>(null)
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")

  const serverValue = server ?? status.data?.serverUrl ?? ""
  const configured = Boolean(connect.data ?? status.data?.configured)
  const linked = status.data?.linked ?? false
  // Quick Connect needs a Seerr newer than v3.3.0 *and* a Quick Connect-enabled
  // Jellyfin server. `link` answers `method: "password"` when either is missing,
  // which is the common case and not an error worth showing.
  const passwordNeeded = link.data?.method === "password" && !link.data.linked
  const error = connect.error ?? link.error ?? linkPassword.error

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Requests through Seerr</DialogTitle>
          <DialogDescription>
            Sign in to Seerr with your own media-server account. MediaFlick keeps the session
            it hands back, never a password and never an instance-wide API key.
          </DialogDescription>
        </DialogHeader>

        {linked ? (
          <div className="flex flex-col gap-2">
            <p className="text-sm">
              Linked as <span className="font-medium">{status.data?.user?.name}</span> on{" "}
              {status.data?.serverUrl}.
            </p>
            <p className="text-xs text-muted-foreground">
              Signing out of Jellyfin, or signing in as somebody else, drops this link.
            </p>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <Label className="flex-col items-start gap-1.5">
              Seerr address
              <Input
                value={serverValue}
                onChange={(event) => setServer(event.target.value)}
                placeholder="https://seerr.example.com"
                autoComplete="url"
              />
            </Label>

            {configured && passwordNeeded && (
              <>
                <Label className="flex-col items-start gap-1.5">
                  Media-server username
                  <Input
                    value={username}
                    onChange={(event) => setUsername(event.target.value)}
                    autoComplete="username"
                  />
                </Label>
                <Label className="flex-col items-start gap-1.5">
                  Password
                  <Input
                    type="password"
                    value={password}
                    onChange={(event) => setPassword(event.target.value)}
                    autoComplete="current-password"
                  />
                </Label>
              </>
            )}

            {status.data?.expired && !error && (
              <p className="text-sm text-destructive">
                The Seerr session has lapsed. Sign in again to keep requesting.
              </p>
            )}
            {error && <p className="text-sm text-destructive">{error.message}</p>}
          </div>
        )}

        <DialogFooter>
          {linked ? (
            <>
              <Button variant="secondary" onClick={onClose}>
                Close
              </Button>
              <Button
                variant="destructive"
                disabled={unlink.isPending}
                onClick={() => unlink.mutate(undefined, { onSuccess: onClose })}
              >
                {unlink.isPending ? "Unlinking…" : "Unlink"}
              </Button>
            </>
          ) : !configured ? (
            <Button
              disabled={connect.isPending || !serverValue.trim()}
              onClick={() =>
                // Probing first is what refuses an instance whose setup wizard
                // is unfinished — signing into one would make this account its
                // owner — and it is where a CSRF-protected Seerr hands out the
                // cookie pair the first write needs.
                connect.mutate(serverValue, { onSuccess: () => link.mutate() })
              }
            >
              {connect.isPending ? "Connecting…" : "Connect"}
            </Button>
          ) : passwordNeeded ? (
            <Button
              disabled={linkPassword.isPending || !username || !password}
              onClick={() =>
                linkPassword.mutate({ username, password }, { onSuccess: onClose })
              }
            >
              {linkPassword.isPending ? "Signing in…" : "Sign in to Seerr"}
            </Button>
          ) : (
            <Button
              disabled={link.isPending}
              onClick={() =>
                link.mutate(undefined, {
                  onSuccess: (result) => {
                    if (result.linked) onClose()
                  },
                })
              }
            >
              {link.isPending ? "Linking…" : "Link"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
