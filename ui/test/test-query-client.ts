import { QueryClient, type QueryClientConfig } from "@tanstack/react-query"

export function testQueryClient(config: QueryClientConfig = {}) {
  const defaults = config.defaultOptions ?? {}
  return new QueryClient({
    ...config,
    defaultOptions: {
      ...defaults,
      queries: {
        gcTime: Infinity,
        retry: false,
        staleTime: Infinity,
        ...defaults.queries,
      },
      mutations: { retry: false, ...defaults.mutations },
    },
  })
}
