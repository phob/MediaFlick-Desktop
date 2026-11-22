import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { api } from "./api"
import { toastFailure } from "./failure-toast"
import { queryKeys } from "./query-client"

export function useWhatsNew(enabled: boolean) {
  return useQuery({ queryKey: queryKeys.whatsNew, queryFn: api.whatsNew, enabled, staleTime: Infinity })
}

export function useCloseWhatsNew() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: api.closeWhatsNew,
    onError: (error) => toastFailure(error),
    onSuccess: (whatsNew) => client.setQueryData(queryKeys.whatsNew, whatsNew),
  })
}
