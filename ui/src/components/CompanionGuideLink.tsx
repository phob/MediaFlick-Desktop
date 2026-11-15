import { ExternalLink } from "lucide-react"
import { Button } from "@/components/ui/button"
import { COMPANION_GUIDE_URL } from "@/lib/companion"

/** Opens the Companion install guide on flick.media in the browser. */
export function CompanionGuideLink() {
  return (
    <Button asChild variant="outline" size="sm">
      <a href={COMPANION_GUIDE_URL} target="_blank" rel="noreferrer">
        <ExternalLink />
        Companion install guide
      </a>
    </Button>
  )
}
