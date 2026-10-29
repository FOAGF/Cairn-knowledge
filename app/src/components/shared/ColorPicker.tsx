// Category colour picker (FR-PRJ-6, D-40): the six preset colours, or any colour chosen with the
// system colour picker or typed as a hex value. Used by the create/edit category dialogs.
import { FolderKanban, Pipette } from 'lucide-react'
import { useId, useState } from 'react'
import { Input } from '@/components/ui/input'
import { useT } from '@/i18n'
import type { CategoryColor } from '@/lib/types'
import { cn } from '@/lib/utils'
import { isPresetColor, PRESET_COLORS, categoryTint } from './tint'

/** `#abc` / `#aabbcc` (the `#` is optional) → lowercase `#aabbcc`; anything else → null. Same rule as Rust. */
export function normalizeHex(text: string): string | null {
  const hex = text.trim().replace(/^#/, '').toLowerCase()
  if (/^[0-9a-f]{6}$/.test(hex)) return `#${hex}`
  if (/^[0-9a-f]{3}$/.test(hex)) return `#${[...hex].map((c) => c + c).join('')}`
  return null
}

const SELECTED = 'ring-2 ring-ring ring-offset-2 ring-offset-background'
/** The custom swatch before a custom colour is chosen: a colour wheel. */
const WHEEL = 'conic-gradient(from 180deg, #ef4444, #f59e0b, #22c55e, #06b6d4, #3b82f6, #a855f7, #ef4444)'

export function ColorPicker({ value, onChange }: { value: CategoryColor; onChange: (color: CategoryColor) => void }) {
  const custom = !isPresetColor(value)
  const [hexText, setHexText] = useState(custom ? value : '')
  const pickerId = useId()
  const customTint = custom ? categoryTint(value) : null
  const t = useT().shared.colorPicker

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        {PRESET_COLORS.map((c) => (
          <button
            key={c}
            type="button"
            title={t.colors[c]}
            aria-label={t.colors[c]}
            onClick={() => {
              setHexText('')
              onChange(c)
            }}
            className={cn('flex size-9 items-center justify-center rounded-xl', categoryTint(c).className, value === c && SELECTED)}
          >
            <FolderKanban className="size-4" />
          </button>
        ))}
        {/* Any colour: the label opens the (visually hidden) system colour picker. */}
        <label
          htmlFor={pickerId}
          title={t.custom}
          className={cn('relative flex size-9 cursor-pointer items-center justify-center rounded-xl', customTint?.className, custom && SELECTED)}
          style={customTint ? customTint.style : { background: WHEEL }}
        >
          {custom ? <FolderKanban className="size-4" /> : <Pipette className="size-4 text-white drop-shadow" />}
          <input
            id={pickerId}
            type="color"
            aria-label={t.custom}
            className="sr-only"
            value={custom ? value : '#3b82f6'}
            onChange={(e) => {
              setHexText(e.target.value)
              onChange(e.target.value)
            }}
          />
        </label>
        <div className="relative">
          {custom && (
            <span className="pointer-events-none absolute top-1/2 left-2.5 size-3 -translate-y-1/2 rounded-full" style={{ backgroundColor: value }} />
          )}
          <Input
            value={hexText}
            placeholder="#e4572e"
            aria-label={t.customHex}
            className={cn('h-9 w-32 font-mono text-sm', custom && 'pl-7')}
            onChange={(e) => {
              setHexText(e.target.value)
              const hex = normalizeHex(e.target.value)
              if (hex) onChange(hex)
            }}
          />
        </div>
      </div>
      <p className="text-xs text-muted-foreground">{t.hint}</p>
    </div>
  )
}
