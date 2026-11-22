import * as React from "react"

function Pressable({ type = "button", ...props }: React.ComponentProps<"button">) {
  return <button type={type} data-slot="pressable" {...props} />
}

export { Pressable }
