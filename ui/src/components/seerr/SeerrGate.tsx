import { Compass } from "lucide-react"
import { useState, type ReactNode } from "react"
import { SeerrSetupDialog } from "@/components/seerr/SeerrSetupDialog"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { useCompanion, useSeerrStatus } from "@/lib/queries"

/**
 * Guards the Seerr views. The sidebar hides them until Seerr is linked, but a
 * deep link, a reload, or a session that lapsed while the app was open all land
 * here — and an unexplained empty page is worse than an offer to set it up.
 */
export function SeerrGate({ children }: { children: ReactNode }) {
  const status = useSeerrStatus()
  // The plugin's own probe-backed query, not the status snapshot: the gate
  // must reflect whether the Companion manages Seerr even when no /api/status
  // refetch has happened since startup.
  const companion = useCompanion()
  const [setup, setSetup] = useState(false)
  const companionManaged =
    companion.data?.compatible &&
    companion.data.info?.capabilities.includes("seerr")

  if (status.isPending) {
    return (
      <div className="p-6">
        <Skeleton className="h-10 w-48" />
      </div>
    )
  }
  if (status.data?.linked) return <>{children}</>

  return (
    <div className="grid h-full place-items-center p-6">
      <Card className="cinematic-panel w-full max-w-md rounded-2xl">
        <CardHeader>
          <div className="mb-2 grid size-12 place-items-center rounded-full bg-primary/15 text-primary">
            <Compass className="size-6" />
          </div>
          <CardTitle className="text-xl">
            {companionManaged
              ? "Your Seerr account is not mapped"
              : status.data?.expired
                ? "The Seerr session has lapsed"
                : "Request through Seerr"}
          </CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <p className="text-sm text-muted-foreground">
            {companionManaged
              ? "Ask your Jellyfin administrator to import this account into Seerr. MediaFlick never needs a separate Seerr login when the Companion plugin is enabled."
              : status.data?.expired
              ? "Sign in to Seerr again to keep requesting movies and series."
              : "Link a Seerr instance to search beyond your library and request what it does not have."}
          </p>
          {status.error && <p className="text-sm text-destructive">{status.error.message}</p>}
          {!companionManaged && (
            <Button className="self-start" onClick={() => setSetup(true)}>
              {status.data?.configured ? "Sign in to Seerr" : "Set up Seerr"}
            </Button>
          )}
        </CardContent>
      </Card>
      {setup && !companionManaged && <SeerrSetupDialog onClose={() => setSetup(false)} />}
    </div>
  )
}
