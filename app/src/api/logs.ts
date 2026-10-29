// The Logs card: the space the logs take and the Detailed logs switch (NFR-SEC-3, D-47).
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { commands } from './bindings'

const KEY = ['logs', 'status'] as const

export function useLogStatus() {
  // The files grow while Cairn runs, so the card asks again every time it opens (the app's default
  // is to never refetch).
  return useQuery({ queryKey: KEY, queryFn: () => commands.logStatus(), staleTime: 0 })
}

/** *Detailed logs* on or off; it lasts until the next start. */
export function useSetDetailedLogs() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (on: boolean) => commands.setDetailedLogs(on),
    onSuccess: (status) => qc.setQueryData(KEY, status),
  })
}
