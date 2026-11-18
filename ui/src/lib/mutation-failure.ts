import type { QueryClient } from "@tanstack/react-query"
import { ApiError } from "./api"
import { toastFailure } from "./failure-toast"
import { queryKeys } from "./query-client"

/**
 * Shared mutation failure handler. Without this a failed action is completely
 * silent — which reads as "nothing happened" and gets clicked again.
 */
export function reportError(client: QueryClient) {
  return (error: Error) => {
    toastFailure(error)
    // The server rejected the token: re-read status so the shell falls back to
    // the sign-in view instead of leaving dead controls on screen.
    if (error instanceof ApiError && (error.expired || error.status === 401)) {
      void client.invalidateQueries({ queryKey: queryKeys.status })
    }
  }
}
