// App-level info: version, data folder, schema version.
import { useQuery } from '@tanstack/react-query'
import { commands } from './bindings'

export function useAppInfo() {
  return useQuery({ queryKey: ['app', 'info'], queryFn: () => commands.appInfo() })
}
