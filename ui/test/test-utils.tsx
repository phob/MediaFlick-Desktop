import { QueryClientProvider, type QueryClient } from "@tanstack/react-query"
import { createContext, useContext, useState, type ReactNode } from "react"
import { createMemoryRouter, RouterProvider, type MemoryRouterProps } from "react-router-dom"

const TestRouteContext = createContext<ReactNode>(null)

function TestRoute() {
  return useContext(TestRouteContext)
}

export function TestProviders({
  children,
  client,
  initialEntries,
}: {
  children: ReactNode
  client: QueryClient
  initialEntries?: MemoryRouterProps["initialEntries"]
}) {
  const [router] = useState(() => createMemoryRouter([{ path: "*", element: <TestRoute /> }], { initialEntries }))
  return (
    <QueryClientProvider client={client}>
      <TestRouteContext.Provider value={children}><RouterProvider router={router} /></TestRouteContext.Provider>
    </QueryClientProvider>
  )
}
