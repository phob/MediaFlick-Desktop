import * as React from "react"
import { ToggleGroup as ToggleGroupPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"

function ToggleGroup({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Root>) {
  return <ToggleGroupPrimitive.Root
    data-slot="toggle-group"
    className={cn("flex w-fit items-center rounded-md border border-input", className)}
    {...props}
  />
}

function ToggleGroupItem({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Item>) {
  return <ToggleGroupPrimitive.Item
    data-slot="toggle-group-item"
    className={cn(
      "inline-flex h-8 min-w-8 shrink-0 items-center justify-center gap-2 border-l border-input bg-transparent px-3 text-sm font-medium whitespace-nowrap outline-none transition-[color,box-shadow] first:rounded-l-md first:border-l-0 last:rounded-r-md hover:bg-muted hover:text-muted-foreground focus:z-10 focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 data-[state=on]:bg-accent data-[state=on]:text-accent-foreground [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
      className,
    )}
    {...props}
  />
}

export { ToggleGroup, ToggleGroupItem }
