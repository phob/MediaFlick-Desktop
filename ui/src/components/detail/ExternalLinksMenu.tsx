import { ExternalLink as ExternalLinkIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import type { ExternalLink, ExternalProvider } from "@/lib/api"

export function ExternalLinksMenu({
  links,
  onSelect,
}: {
  links: readonly ExternalLink[]
  onSelect?: (provider: ExternalProvider) => void
}) {
  if (!links.length) return null

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="secondary" size="lg">
          <ExternalLinkIcon className="size-4" />
          More info
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {links.map((link) =>
          link.kind === "url" ? (
            <DropdownMenuItem key={link.id} asChild>
              <a href={link.href} target="_blank" rel="noreferrer">
                <ExternalLinkIcon className="size-4" />
                {link.actionLabel ?? `View on ${link.label}`}
              </a>
            </DropdownMenuItem>
          ) : (
            <DropdownMenuItem key={link.id} onSelect={() => onSelect?.(link.id)}>
              <ExternalLinkIcon className="size-4" />
              View on {link.label}
            </DropdownMenuItem>
          ),
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
