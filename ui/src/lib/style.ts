import type { CSSProperties } from "react"

/** React style props plus the application-owned CSS custom properties they carry. */
export interface CSSVariableProperties extends CSSProperties {
  [name: `--${string}`]: string | number | undefined
}

export function cssVariables(properties: CSSVariableProperties): CSSVariableProperties {
  return properties
}
