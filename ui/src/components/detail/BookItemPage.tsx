import { BookOpen, RotateCcw } from "lucide-react"
import { useEffect, useRef } from "react"
import { Link } from "react-router-dom"
import { GenericItemPage } from "@/components/detail/GenericItemPage"
import { Button } from "@/components/ui/button"
import type { BookItemDetail, ItemAbout, ReadingState } from "@/lib/api"
import type { DetailNavigationState } from "@/lib/navigation"
import { readerPath } from "@/reader/route"

function readingFact(reading: ReadingState) {
  switch (reading.state) {
    case "notStarted":
      return "Not started"
    case "inProgress":
      return reading.percent ? `${reading.percent}% read` : "Started"
    case "finished":
      return "Finished"
  }
}

function primaryAction(reading: ReadingState) {
  switch (reading.state) {
    case "notStarted":
      return { label: "Read", entry: "resume" } as const
    case "inProgress":
      return { label: reading.percent ? `Continue at ${reading.percent}%` : "Continue", entry: "resume" } as const
    case "finished":
      return { label: "Read again", entry: "start" } as const
  }
}

export function BookItemPage({
  item,
  about,
  navigationState,
}: {
  item: BookItemDetail
  about: ItemAbout | undefined
  navigationState: DetailNavigationState
}) {
  const primary = primaryAction(item.reading)
  const primaryLink = useRef<HTMLAnchorElement>(null)
  useEffect(() => primaryLink.current?.focus({ preventScroll: true }), [])
  return (
    <GenericItemPage item={item} about={about} navigationState={navigationState} facts={[readingFact(item.reading)]}>
      <div className="flex flex-wrap items-center gap-3">
        <Button asChild size="lg">
          <Link ref={primaryLink} to={readerPath(item.id, primary.entry)} state={navigationState}>
            <BookOpen aria-hidden />
            {primary.label}
          </Link>
        </Button>
        {item.reading.state === "inProgress" && (
          <Button asChild size="lg" variant="secondary">
            <Link to={readerPath(item.id, "start")} state={navigationState}>
              <RotateCcw aria-hidden />
              Read from the start
            </Link>
          </Button>
        )}
      </div>
    </GenericItemPage>
  )
}
