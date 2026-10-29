import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { createConversation, restoreConversation, updateConversation, useConversationList, useMessages } from '@/api/conversations'
import { effortLevels, getModelsSnapshot, getProvidersSnapshot, useModels, useProviders, usableModels } from '@/api/providers'
import { readSettings, useSettings } from '@/api/settings'
import { Archive, ArchiveRestore, Sparkles, UploadCloud } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { TintIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { timeUntil } from '@/lib/format'
import type { Effort } from '@/lib/types'
import { fileFromDrop, isExternalFileDrag } from '@/lib/dropGuard'
import { cn } from '@/lib/utils'
import { useBusy } from './busy'
import { Composer, type EffortChoice } from './Composer'
import { ConversationList } from './ConversationList'
import { ATTACH_ACCEPT, draftCategoryId, fileTypeError, NEW_FILE_TITLE, sendFile } from './fileDraft'
import { activeDraftMessage, askedWeb, isRunning } from './messages'
import { MessageView, UploadingIndicator } from './MessageView'
import { sendChatMessage } from './send'
import { templateParts, templates } from './suggestions'

export default function AssistantPage() {
  const { conversationId } = useParams()
  const [params, setParams] = useSearchParams()
  const navigate = useNavigate()
  const conversations = useConversationList()
  const modelsQuery = useModels()
  const providersQuery = useProviders()
  const models = useMemo(() => modelsQuery.data ?? [], [modelsQuery.data])
  const providers = useMemo(() => providersQuery.data ?? [], [providersQuery.data])
  const providersLoaded = modelsQuery.isSuccess && providersQuery.isSuccess
  const defaultModelId = useSettings().defaultModelId
  const uploadingMap = useBusy((s) => s.uploading)
  const t = useT()
  const a = t.assistant.page

  const conv = conversations.find((c) => c.id === conversationId)
  // FR-LLM-AS-13: an archived conversation is read-only until restored.
  const archived = !!conv?.archivedAt
  const messagesQuery = useMessages(conv?.id)
  const messages = useMemo(() => messagesQuery.data ?? [], [messagesQuery.data])
  // D-67: a conversation keeps the model and effort picked in it; a new one starts from the default
  // model and is given what was picked (or the default) when it is created.
  const [draftModelId, setDraftModelId] = useState<string>()
  const [draftEffort, setDraftEffort] = useState<EffortChoice>('unset')
  const modelId = (conv ? conv.modelId : draftModelId) ?? defaultModelId
  const effort: EffortChoice = conv ? (conv.effort ?? 'unset') : draftEffort
  const setModelId = (id: string) => (conv ? void updateConversation(conv.id, { modelId: id }).catch((e) => toast.error(errorMessage(e))) : setDraftModelId(id))
  const setEffort = (e: EffortChoice) =>
    conv ? void updateConversation(conv.id, { effort: e === 'unset' ? '' : e }).catch((err) => toast.error(errorMessage(err))) : setDraftEffort(e)
  const [web, setWeb] = useState(false) // 🌐, for the next message only (FR-LLM-AS-12)
  const [draftScope, setDraftScope] = useState<string[]>([]) // scope before the conversation exists
  // Models of disabled providers are not offered; fall back to the first enabled one.
  const enabledModels = useMemo(() => usableModels(providers, models), [models, providers])
  const selected = models.find((m) => m.id === modelId)
  const selectedProvider = providers.find((p) => p.id === selected?.providerId)
  const fallenBackFrom = selectedProvider && !selectedProvider.enabled ? selectedProvider.name : undefined
  const model = fallenBackFrom || !selected ? enabledModels[0] : selected
  // While the lists load there is nothing to complain about yet.
  const noProvider = providersLoaded && enabledModels.length === 0
  const scope = conv ? conv.scopeCategoryIds : draftScope
  // D-64: a template picked on the empty conversation goes into the message box (a new object per click).
  const [template, setTemplate] = useState<{ text: string }>()
  // Busy: a job is working (state on the stored message), or a file is on its way to Rust. By the URL's id,
  // so a file sent from the drop zone shows its upload before the new conversation reaches the list.
  const uploading = conversationId ? uploadingMap[conversationId] : undefined
  const busy = messages.some(isRunning) || uploading !== undefined
  // A level chosen for another model is ignored if this model doesn't offer it.
  const effortToSend: Effort | undefined = effort !== 'unset' && effortLevels(model).includes(effort) ? effort : undefined

  // File attached in the composer or dropped on the chat (FR-KN-UP-1..3).
  const [file, setFile] = useState<File | null>(null)
  const [fileError, setFileError] = useState('')
  const [dragging, setDragging] = useState(false)
  const dragDepth = useRef(0)
  const input = useRef<HTMLInputElement>(null)
  const dropZone = useRef<HTMLButtonElement>(null)
  const pickFile = (f: File | undefined | null) => {
    if (!f) return
    const err = fileTypeError(f)
    setFileError(err ?? '')
    if (!err) setFile(f)
  }
  const hasFiles = (e: React.DragEvent) => isExternalFileDrag(e)

  // "Create knowledge from a file" (/assistant?attach=…) shows the drop zone. Like a new conversation, it is
  // only created when the first message is sent. An empty one saved by an earlier version opens the same way.
  const attach = params.get('attach')
  const fileEmpty = conversationId
    ? !!conv && conv.title === NEW_FILE_TITLE && messagesQuery.isSuccess && messages.length === 0
    : attach !== null
  // 🌐 can't go with a file, and a message while a draft card is active refines the draft.
  const webAllowed = !file && !fileEmpty && !activeDraftMessage(messages)

  const send = async (text: string, attached: File | null = null) => {
    if (!model) return
    try {
      let id = conv?.id
      if (!id) {
        // Rust titles the conversation from its first message (`send_message`); a file draft is titled
        // after its file (`sendFile`).
        const title = attached && fileEmpty ? NEW_FILE_TITLE : 'New conversation' // i18n-ignore: stored title, Rust replaces it
        id = (await createConversation({ title, scopeCategoryIds: draftScope, modelId: model.id, effort: effortToSend ?? null })).id
        // The next new conversation starts from the default model again.
        setDraftModelId(undefined)
        setDraftEffort('unset')
        navigate(`/assistant/${id}`)
      }
      const useWeb = web && !!model.webSearch && webAllowed
      setWeb(false)
      if (attached) {
        setFile(null)
        setFileError('')
      }
      // The empty "Create knowledge from a file" conversation drafts a page (M5); elsewhere a file is part of a normal message.
      const job =
        attached && fileEmpty
          ? sendFile(id, attached, text, model.id, draftCategoryId(conv ?? { scopeCategoryIds: draftScope }), effortToSend)
          : sendChatMessage(id, text, { modelId: model.id, effort: effortToSend, file: attached, web: useWeb })
      void job.catch((e) => toast.error(errorMessage(e)))
    } catch (e) {
      toast.error(errorMessage(e))
    }
  }

  // Home "Ask anything" bar: /assistant?q=… starts a conversation once (StrictMode-safe).
  const handledQ = useRef<string | null>(null)
  const q = params.get('q')
  useEffect(() => {
    if (!q || handledQ.current === q || !providersLoaded) return
    handledQ.current = q
    // Optional ?scope=<categoryId> (from a category page) limits the search (FR-LLM-AS-4).
    const scope = params.get('scope')
    const usableList = usableModels(getProvidersSnapshot(), getModelsSnapshot())
    const def = usableList.find((m) => m.id === readSettings().defaultModelId)
    const usable = def ?? usableList[0]
    // i18n-ignore: stored title, Rust replaces it
    void createConversation({ title: 'New conversation', scopeCategoryIds: scope ? [scope] : [], modelId: usable?.id ?? null })
      .then(({ id }) => {
        navigate(`/assistant/${id}`, { replace: true })
        if (usable) void sendChatMessage(id, q, { modelId: usable.id }).catch((e) => toast.error(errorMessage(e)))
      })
      .catch((e) => toast.error(errorMessage(e)))
  }, [q, params, providersLoaded, navigate, setParams])

  // "Create from file" entries open the file picker once per ?attach value (StrictMode-safe; the
  // Assistant's own entry sends a new value on every click).
  const handledAttach = useRef<string | null>(null)
  useEffect(() => {
    if (!attach || conversationId || handledAttach.current === attach) return
    handledAttach.current = attach
    input.current?.click() // browsers may block this without a recent click; the drop zone is focused as a fallback
    dropZone.current?.focus()
  }, [attach, conversationId])

  // Keep the newest message in view.
  const bottom = useRef<HTMLDivElement>(null)
  const last = messages.at(-1)
  const lastText = last?.text
  const lastSteps = last?.steps.length
  useEffect(() => {
    bottom.current?.scrollIntoView({ block: 'end' })
  }, [messages.length, lastText, lastSteps, busy])

  const showEmpty = !conv || (messagesQuery.isSuccess && messages.length === 0)

  return (
    // The whole Assistant takes a dropped file (FR-KN-UP-7), the conversation list included.
    <div
      className="grid h-[calc(100vh-7.5rem)] min-h-[520px] gap-4 lg:grid-cols-[290px_1fr]"
      onDragEnter={(e) => { if (!hasFiles(e)) return; e.preventDefault(); dragDepth.current++; setDragging(true) }}
      onDragOver={(e) => { if (!hasFiles(e)) return; e.preventDefault(); e.dataTransfer.dropEffect = 'copy' }}
      onDragLeave={(e) => { if (!hasFiles(e)) return; dragDepth.current = Math.max(0, dragDepth.current - 1); if (!dragDepth.current) setDragging(false) }}
      onDrop={(e) => {
        if (!hasFiles(e)) return
        e.preventDefault()
        dragDepth.current = 0
        setDragging(false)
        void fileFromDrop(e.dataTransfer).then(
          (file) => {
            if (!file) return void toast.error(a.dropUnreadable)
            if (!busy && !noProvider && !archived) pickFile(file)
          },
          (err) => toast.error(errorMessage(err)),
        )
      }}
    >
      <ConversationList activeId={conv?.id} />

      <section className="relative flex min-h-0 flex-col rounded-2xl border bg-card/40 p-4">
        <input ref={input} type="file" hidden accept={ATTACH_ACCEPT} data-testid="attach-input" onChange={(e) => { pickFile(e.target.files?.[0]); e.target.value = '' }} />
        {dragging && (
          <div className="pointer-events-none absolute inset-2 z-10 flex flex-col items-center justify-center rounded-2xl border-2 border-dashed border-primary bg-tint-blue/80 text-tint-blue-fg backdrop-blur-[1px]">
            <UploadCloud className="mb-2 size-9" />
            <p className="text-lg font-semibold">{a.dropToAttach}</p>
            {/* i18n-ignore: file extensions */}
            <p className="text-sm opacity-80">.md .txt .pdf .docx .png .jpg .jpeg</p>
          </div>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto">
          {fileEmpty && !busy ? (
            <div className="mx-auto flex h-full max-w-2xl flex-col items-center justify-center pb-6 text-center">
              <button
                ref={dropZone}
                type="button"
                onClick={() => input.current?.click()}
                className="flex w-full flex-col items-center rounded-3xl border-2 border-dashed bg-card/60 px-6 py-12 transition-colors hover:border-primary hover:bg-tint-blue/30 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none"
              >
                <TintIcon icon={UploadCloud} tint="purple" size="lg" className="mb-4" />
                <span className="text-2xl font-semibold tracking-tight">{a.dropOrBrowse}</span>
                <span className="mt-2 text-sm text-muted-foreground">{a.noSizeLimit}</span>
              </button>
              <p className="mt-4 max-w-md text-sm text-muted-foreground">{a.fileHint}</p>
            </div>
          ) : showEmpty && !busy ? (
            <div className="mx-auto flex h-full max-w-2xl flex-col items-center justify-center pb-6 text-center">
              <TintIcon icon={Sparkles} tint="blue" size="lg" className="mb-4" />
              <h1 className="text-2xl font-semibold tracking-tight">{a.emptyTitle}</h1>
              <p className="mt-1 text-muted-foreground">{a.emptyText}</p>
              {/* D-64: templates; a click puts one into the message box to be filled in, nothing is sent. */}
              <div className="mt-8 grid w-full gap-3 sm:grid-cols-2">
                {templates().map((s) => (
                  <button
                    key={s.label}
                    onClick={() => setTemplate({ text: s.text })}
                    className="flex items-center gap-3 rounded-2xl border bg-card p-4 text-left transition-shadow hover:shadow-md"
                  >
                    <TintIcon icon={s.icon} tint={s.tint} />
                    <span className="min-w-0">
                      <span className="block text-xs text-muted-foreground">{s.label}</span>
                      <span className="block text-sm font-medium">
                        {templateParts(s.text).map((part, i) =>
                          part.blank ? (
                            <span key={i} className="rounded bg-tint-blue px-1 font-normal text-tint-blue-fg">
                              {part.text.slice(1, -1)}
                            </span>
                          ) : (
                            <span key={i}>{part.text}</span>
                          ),
                        )}
                      </span>
                    </span>
                  </button>
                ))}
              </div>
              <p className="mt-4 text-sm text-muted-foreground">{a.templatesHint}</p>
            </div>
          ) : (
            // A disabled fieldset turns off every button in the messages of an archived conversation
            // (retry, draft cards, proposals); links still open.
            <fieldset disabled={archived} className="mx-auto min-w-0 max-w-3xl space-y-6 px-2 py-2">
              {messages.map((m, i) => (
                <MessageView key={m.id} message={m} web={m.role === 'user' && askedWeb(messages[i + 1])} />
              ))}
              {uploading !== undefined && <UploadingIndicator fraction={uploading} />}
              <div ref={bottom} />
            </fieldset>
          )}
        </div>

        <div className={cn('mx-auto w-full max-w-3xl pt-3')}>
          {archived && conv ? (
            <div className="flex items-center justify-between gap-3 rounded-2xl border bg-muted/60 px-4 py-3">
              <span className="flex items-center gap-2 text-sm text-muted-foreground">
                <Archive className="size-4 shrink-0" />
                {a.archived(conv.expiresAt ? timeUntil(conv.expiresAt) : null)}
              </span>
              <Button size="sm" onClick={() => void restoreConversation(conv.id).catch((e) => toast.error(errorMessage(e)))}>
                <ArchiveRestore /> {t.common.restore}
              </Button>
            </div>
          ) : (
          <Composer
            conversationId={conv?.id}
            web={web}
            onWebChange={setWeb}
            webAllowed={webAllowed}
            modelId={model?.id ?? ''}
            onModelChange={setModelId}
            effort={effort}
            onEffortChange={setEffort}
            scope={scope}
            onScopeChange={(ids) => (conv ? void updateConversation(conv.id, { scopeCategoryIds: ids }) : setDraftScope(ids))}
            disabled={busy || noProvider}
            noProvider={noProvider}
            disabledProviderName={fallenBackFrom}
            template={template}
            file={file}
            onFileChange={(f) => { setFile(f); setFileError('') }}
            onBrowse={() => input.current?.click()}
            fileError={fileError}
            onSend={send}
          />
          )}
        </div>
      </section>
    </div>
  )
}
