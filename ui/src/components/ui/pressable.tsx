import * as React from "react"

/**
 * A bare `<button>` with no look of its own, for controls whose whole look is
 * their content: artwork tiles, rail arrows, slide ticks, card actions. It
 * defaults to `type="button"`. A control that should look like a button uses
 * `Button` instead; callers keep their own focus-visible ring.
 */
function Pressable({ type = "button", ...props }: React.ComponentProps<"button">) {
  return <button type={type} data-slot="pressable" {...props} />
}

export { Pressable }
