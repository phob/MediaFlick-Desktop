import { toast } from "sonner"
import { ApiError } from "./api"

export function toastFailure(error: Error, message = error.message) {
  if (error instanceof ApiError && error.accountChanged) return
  toast.error(message)
}
