import { Construction } from 'lucide-react'
import { useT } from '@/i18n'
import { EmptyState } from './PageHeader'

export function Placeholder({ name }: { name: string }) {
  const t = useT()
  return (
    <EmptyState icon={Construction} title={name}>
      {t.shared.placeholder}
    </EmptyState>
  )
}
