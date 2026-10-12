import { Button } from "@/components/ui/button"
import { useTouchInput } from "@/hooks/use-touch-input"
import type { LetterIndex, LibraryLetter } from "@/lib/api"
import { cn } from "@/lib/utils"

const LETTERS: readonly LibraryLetter[] = [
  "#", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
  "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
]

export function LetterJumpRail({
  letters,
  onJump,
}: {
  letters: LetterIndex
  onJump: (index: number) => void
}) {
  const touchInput = useTouchInput()
  return (
    <nav
      aria-label="Jump to letter"
      className="flex shrink-0 flex-col items-center justify-center-safe overflow-y-auto py-5 pr-1"
    >
      {LETTERS.map((letter) => {
        const index = letters[letter]
        return (
          <Button
            key={letter}
            type="button"
            variant="ghost"
            size="xs"
            aria-label={`Jump to ${letter}`}
            disabled={index === undefined}
            onClick={() => {
              if (index !== undefined) onJump(index)
            }}
            className={cn(
              "data-label h-5 w-7 shrink-0 px-0 text-muted-foreground hover:text-primary",
              touchInput && "min-h-11 min-w-11",
            )}
          >
            {letter}
          </Button>
        )
      })}
    </nav>
  )
}
