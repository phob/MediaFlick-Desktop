import { useEffect, useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { LogOut, RefreshCw } from "lucide-react"
import { CoverCard } from "@/components/CoverCard"
import { Button } from "@/components/ui/button"
import { api, type SyncRetry } from "@/lib/api"
import { toastFailure } from "@/lib/failure-toast"
import { useLogout } from "@/lib/session-queries"
import { queryKeys } from "@/lib/query-client"
import { SYNC_FAILURES } from "@/lib/sync-failure"

function useUnixSeconds() {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1_000))
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Math.floor(Date.now() / 1_000)), 1_000)
    return () => window.clearInterval(timer)
  }, [])
  return now
}

/**
 * Shown instead of Home when the first catalog page failed. The sync worker
 * keeps retrying on its own; this says why it is waiting, when it tries next,
 * and offers the two ways out.
 */
export function BootstrapRetry({ retry, attempting }: { retry: SyncRetry; attempting: boolean }) {
  const client = useQueryClient()
  const logout = useLogout()
  const now = useUnixSeconds()
  const request = useMutation({
    mutationFn: api.sync,
    onSuccess: () => client.invalidateQueries({ queryKey: queryKeys.status }),
    onError: (error: Error) => toastFailure(error),
  })
  const failure = SYNC_FAILURES[retry.reason]
  const seconds = retry.retryAt - now
  const trying = attempting || request.isPending || seconds <= 0

  return (
    <CoverCard title={failure.title} description={failure.detail}>
      <p className="text-sm text-muted-foreground">
        {trying ? "Trying again now…" : `Trying again in ${seconds} s.`}
      </p>
      <div className="flex flex-col gap-2 sm:flex-row">
        <Button className="h-11 sm:flex-1" disabled={trying} onClick={() => request.mutate()}>
          <RefreshCw className={trying ? "animate-spin" : undefined} />
          Retry now
        </Button>
        <Button
          variant="outline"
          className="h-11 sm:flex-1"
          disabled={logout.isPending}
          onClick={() => logout.mutate()}
        >
          <LogOut />
          Sign out
        </Button>
      </div>
    </CoverCard>
  )
}
