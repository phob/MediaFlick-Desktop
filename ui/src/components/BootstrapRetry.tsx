import { useEffect, useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { LogOut, RefreshCw } from "lucide-react"
import appIcon from "../../../distribution/app-icon.png?inline"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { api, type SyncRetry } from "@/lib/api"
import { toastFailure } from "@/lib/failure-toast"
import { useLogout } from "@/lib/queries"
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
    <div className="signin-page relative grid h-full place-items-center overflow-x-hidden overflow-y-auto p-6">
      <div className="relative z-10 flex w-full max-w-md flex-col gap-7">
        <img
          src={appIcon}
          alt="MediaFlick"
          className="mx-auto size-28 drop-shadow-[0_1.25rem_2rem_rgb(0_0_0/45%)]"
        />
        <Card className="cinematic-panel w-full gap-5 rounded-2xl py-7">
          <CardHeader className="gap-2">
            <CardTitle role="heading" aria-level={1} className="text-xl">
              {failure.title}
            </CardTitle>
            <CardDescription>{failure.detail}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
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
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
