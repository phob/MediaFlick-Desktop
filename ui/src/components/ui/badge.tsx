import * as React from "react"
import { Slot } from "radix-ui"

import { cn } from "@/lib/utils"

const BADGE_BASE = "inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-md border px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-[color,box-shadow] focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 [&>svg]:pointer-events-none [&>svg]:size-3"
const BADGE_VARIANTS = {
  default: "border-transparent bg-primary text-primary-foreground",
  secondary: "border-transparent bg-secondary text-secondary-foreground",
  destructive: "border-transparent bg-destructive text-white",
  outline: "text-foreground",
} as const

function Badge({
  className,
  variant = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"span"> & {
  variant?: keyof typeof BADGE_VARIANTS
  asChild?: boolean
}) {
  const Comp = asChild ? Slot.Root : "span"

  return (
    <Comp data-slot="badge" className={cn(BADGE_BASE, BADGE_VARIANTS[variant], className)} {...props} />
  )
}

export { Badge }
