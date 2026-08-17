import type { ReactNode } from "react"
import { RatingsProvider } from "@/lib/ratings"
import { TechnicalProvider } from "@/lib/technical"

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <RatingsProvider>
      <TechnicalProvider>{children}</TechnicalProvider>
    </RatingsProvider>
  )
}
