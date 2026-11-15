import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { api } from "./api"
import { toastFailure } from "./failure-toast"
import { queryKeys } from "./query-client"

/** The one-time What's new for a device updated from 0.1.6. */
export function useWhatsNew(enabled: boolean) {
  return useQuery({ queryKey: queryKeys.whatsNew, queryFn: api.whatsNew, enabled, staleTime: Infinity })
}

/** Records it as seen; a failure leaves it due again on the next start. */
export function useCloseWhatsNew() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: api.closeWhatsNew,
    onError: (error) => toastFailure(error),
    onSuccess: (whatsNew) => client.setQueryData(queryKeys.whatsNew, whatsNew),
  })
}
