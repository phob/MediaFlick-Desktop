import { useEffect } from "react"
import { RefreshCw } from "lucide-react"
import { useRouteError } from "react-router-dom"
import { CoverCard } from "@/components/CoverCard"
import { Button } from "@/components/ui/button"
import { formatRootErrorSummary, reloadApp } from "@/lib/root-error"
import { signalWindowReady, windowRevealed } from "@/lib/startup"

export function RootErrorScreen({ onReload = reloadApp }: { onReload?: () => void } = {}) {
  const error = useRouteError()
  const summary = formatRootErrorSummary(error)

  useEffect(() => {
    if (!windowRevealed()) signalWindowReady()
  }, [])

  return (
    <CoverCard
      title="Something went wrong"
      description="MediaFlick ran into an unexpected error while rendering the interface."
    >
      <p className="rounded-lg border border-border bg-muted/50 px-3 py-2 text-xs break-words text-muted-foreground select-text">
        {summary}
      </p>
      <Button className="h-11 w-full" onClick={onReload}>
        <RefreshCw />
        Reload MediaFlick
      </Button>
    </CoverCard>
  )
}
