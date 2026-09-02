import { QueryClientProvider, type QueryClient } from "@tanstack/react-query"
import type { ReactNode } from "react"
import { MemoryRouter, type MemoryRouterProps } from "react-router-dom"

export function TestProviders({
  children,
  client,
  initialEntries,
}: {
  children: ReactNode
  client: QueryClient
  initialEntries?: MemoryRouterProps["initialEntries"]
}) {
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={initialEntries}>{children}</MemoryRouter>
    </QueryClientProvider>
  )
}
