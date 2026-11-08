import { GenericItemPage } from "@/components/detail/GenericItemPage"
import type { BookItemDetail, ItemAbout, ReadingState } from "@/lib/api"
import type { DetailNavigationState } from "@/lib/navigation"

function readingLabel(reading: ReadingState) {
  switch (reading.state) {
    case "notStarted":
      return "Not started"
    case "inProgress":
      return reading.percent == null ? "In progress" : `${reading.percent}% read`
    case "finished":
      return "Finished"
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
  return <GenericItemPage item={item} about={about} navigationState={navigationState} facts={[readingLabel(item.reading)]} />
}
