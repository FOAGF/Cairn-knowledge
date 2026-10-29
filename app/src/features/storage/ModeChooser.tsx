// FR-KN-EXT-7: where knowledge pages live, chosen once on a new data folder. It fills the window, comes
// before everything else (the first-start card included) and can't be skipped. Settings › Backup &
// data can switch later, and every switch keeps a backup of the knowledge pages first (FR-KN-EXT-8).
import { Database, FlaskConical, FolderOpen, Info, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { commands, type KnowledgeMode } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { chooseKnowledgeMode } from '@/api/storage'
import { CairnLogo } from '@/components/shared/CairnLogo'
import { TintIcon } from '@/components/shared/tint'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { useT } from '@/i18n'
import { pickFolder } from '@/lib/pickFolder'
import { cn } from '@/lib/utils'

// `items-stretch`: the Label is `items-center`, which would let a long folder path widen the card.
const CARD = 'flex min-w-0 cursor-pointer flex-col items-stretch gap-3 rounded-2xl border bg-card p-5 font-normal leading-normal shadow-[0_1px_2px_rgba(16,24,40,0.04)] transition-colors hover:border-primary/60' // i18n-ignore: class names

export function ModeChooser() {
  const [mode, setMode] = useState<KnowledgeMode | null>(null)
  const [root, setRoot] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const ready = mode === 'internal' || (mode === 'external' && !!root)
  const t = useT()
  const m = t.storage.modeChooser

  const takeFolder = (folder: string) => {
    setRoot(folder)
    setMode('external')
    setError('')
  }
  const choose = async () => {
    const folder = await pickFolder(t.storage.pickFolder.knowledge)
    if (folder) takeFolder(folder)
  }
  // Development builds only: a copy of the sample folder (categories, nested groups, images, …).
  const sample = () => void commands.devSampleKnowledgeFolder().then(takeFolder, (e) => setError(errorMessage(e)))
  const start = async () => {
    if (!ready) return
    setBusy(true)
    setError('')
    try {
      // The settings change, and the app shell shows the app.
      await chooseKnowledgeMode(mode, mode === 'external' ? root : null)
    } catch (e) {
      setError(errorMessage(e))
      setBusy(false)
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-6 py-10" data-testid="mode-chooser">
      <div className="w-full max-w-4xl space-y-6">
        <div className="flex items-center gap-3">
          <CairnLogo className="size-10" />
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">{m.welcome}</h1>
            <p className="text-sm text-muted-foreground">{m.intro}</p>
          </div>
        </div>

        <RadioGroup value={mode ?? ''} onValueChange={(v) => setMode(v as KnowledgeMode)} className="grid gap-4 md:grid-cols-2" aria-label={m.aria}>
          <Label className={cn(CARD, mode === 'internal' && 'border-primary ring-2 ring-primary/20')} data-testid="mode-internal">
            <div className="flex items-start gap-3">
              <TintIcon icon={Database} tint="blue" />
              <div className="min-w-0 flex-1">
                <p className="text-base font-semibold">{m.internal}</p>
                <p className="text-sm text-muted-foreground">{m.internalBody}</p>
              </div>
              <RadioGroupItem value="internal" className="mt-1" aria-label={m.internal} />
            </div>
            <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
              <li>{m.internalTogether}</li>
              <li>{m.internalOtherPrograms}</li>
            </ul>
          </Label>

          <Label className={cn(CARD, mode === 'external' && 'border-primary ring-2 ring-primary/20')} data-testid="mode-external">
            <div className="flex items-start gap-3">
              <TintIcon icon={FolderOpen} tint="amber" />
              <div className="min-w-0 flex-1">
                <p className="text-base font-semibold">{m.external}</p>
                <p className="text-sm text-muted-foreground">{m.externalBody}</p>
              </div>
              <RadioGroupItem value="external" className="mt-1" aria-label={m.external} />
            </div>
            <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
              <li>{m.externalFolders}</li>
              <li>{m.externalTools}</li>
              <li>{m.externalBackups}</li>
            </ul>
            <div className="mt-auto min-w-0 space-y-2 border-t pt-3">
              <div className="flex min-w-0 items-center gap-2">
                <div className="min-w-0 flex-1 rounded-lg border bg-muted/40 px-2.5 py-1.5 font-mono text-xs break-all" title={root ?? undefined} data-testid="mode-root">
                  {root ?? m.noFolder}
                </div>
                <Button type="button" variant="outline" size="sm" onClick={() => void choose()} data-testid="mode-pick-folder">
                  <FolderOpen /> {root ? m.change : m.chooseFolder}
                </Button>
              </div>
              {import.meta.env.DEV && (
                <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" onClick={sample} data-testid="mode-sample-folder">
                  <FlaskConical /> {m.sample}
                </Button>
              )}
            </div>
          </Label>
        </RadioGroup>

        <Alert>
          <Info />
          <AlertTitle>{m.laterTitle}</AlertTitle>
          <AlertDescription>
            {m.laterBody}
          </AlertDescription>
        </Alert>

        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <div className="flex justify-end">
          <Button size="lg" disabled={!ready || busy} onClick={() => void start()} data-testid="mode-continue">
            {busy && <Loader2 className="animate-spin" />}
            {mode === 'external' ? (root ? m.useFolder : m.chooseFirst) : mode === 'internal' ? m.keepInCairn : m.chooseOne}
          </Button>
        </div>
      </div>
    </div>
  )
}
