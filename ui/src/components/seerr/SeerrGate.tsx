import { Compass } from "lucide-react"
import type { ReactNode } from "react"
import { Link } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { ApiError } from "@/lib/api"
import { useCompanion, useSeerrStatus } from "@/lib/queries"

/**
 * Guards the Seerr views. The sidebar hides them without the Companion
 * capability, while deep links and unmapped users land on an explanation.
 */
export function SeerrGate({ children }: { children: ReactNode }) {
  // The plugin's own probe-backed query, not the status snapshot: the gate
  // must reflect whether the Companion manages Seerr even when no /api/status
  // refetch has happened since startup.
  const companion = useCompanion()
  const companionManaged =
    companion.data?.compatible &&
    companion.data.info?.capabilities.includes("seerr")
  const status = useSeerrStatus(Boolean(companionManaged))

  if (companion.isPending || (companionManaged && status.isPending)) {
    return (
      <div className="p-6">
        <Skeleton className="h-10 w-48" />
      </div>
    )
  }
  if (status.data?.linked) return <>{children}</>

  const mappingMissing =
    companionManaged &&
    status.error instanceof ApiError &&
    status.error.status === 409

  return (
    <div className="grid h-full place-items-center p-6">
      <Card className="cinematic-panel w-full max-w-md rounded-2xl">
        <CardHeader>
          <div className="mb-2 grid size-12 place-items-center rounded-full bg-primary/15 text-primary">
            <Compass className="size-6" />
          </div>
          <CardTitle className="text-xl">
            {mappingMissing ? "Your Seerr account is not mapped" : "Seerr is unavailable"}
          </CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <p className="text-sm text-muted-foreground">
            {mappingMissing
              ? "Ask your Jellyfin administrator to import this account into Seerr."
              : companionManaged
                ? "Seerr is unavailable for this account."
                : "This Jellyfin server does not provide Seerr discovery and requests."}
          </p>
          {status.error && <p className="text-sm text-destructive">{status.error.message}</p>}
          <Button asChild className="self-start">
            <Link to="/">Back to Home</Link>
          </Button>
        </CardContent>
      </Card>
    </div>
  )
}
