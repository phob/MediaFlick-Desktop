import type { CSSProperties } from "react"

export interface CSSVariableProperties extends CSSProperties {
  [name: `--${string}`]: string | number | undefined
}
