import { ExternalLink } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import type { ExternalMenuLink, ExternalProvider } from "@/lib/api"

export function ExternalLinksMenu({
  links,
  onSelect,
}: {
  links: readonly ExternalMenuLink[]
  onSelect?: (provider: ExternalProvider) => void
}) {
  if (!links.length) return null

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="secondary" size="lg">
          <ExternalLink className="size-4" />
          More info
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {links.map((link) =>
          "href" in link ? (
            <DropdownMenuItem key={link.id} asChild>
              <a href={link.href} target="_blank" rel="noreferrer">
                <ExternalLink className="size-4" />
                {link.actionLabel ?? `View on ${link.label}`}
              </a>
            </DropdownMenuItem>
          ) : (
            <DropdownMenuItem key={link.id} onSelect={() => onSelect?.(link.id)}>
              <ExternalLink className="size-4" />
              View on {link.label}
            </DropdownMenuItem>
          ),
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
