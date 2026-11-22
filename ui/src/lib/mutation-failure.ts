import type { QueryClient } from "@tanstack/react-query"
import { ApiError } from "./api"
import { toastFailure } from "./failure-toast"
import { queryKeys } from "./query-client"

export function reportError(client: QueryClient) {
  return (error: Error) => {
    toastFailure(error)
    if (error instanceof ApiError && (error.expired || error.status === 401)) {
      void client.invalidateQueries({ queryKey: queryKeys.status })
    }
  }
}
