import { toast } from "sonner"
import { ApiError } from "./api"

/**
 * Shows why a request failed. A request that outlived an account switch stays
 * silent: the account it belonged to is gone, and the new account's state was
 * already reset.
 */
export function toastFailure(error: Error, message = error.message) {
  if (error instanceof ApiError && error.accountChanged) return
  toast.error(message)
}
