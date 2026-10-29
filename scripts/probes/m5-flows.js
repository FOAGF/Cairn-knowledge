// M5 creation flows end to end in the real app, on the dev sample data, with the real sidecar (file
// extraction, search model) and a mock LLM (`scripts/mock_llm.py`, port MOCK_LLM_PORT, default 5298):
//   python3 -I scripts/mock_llm.py 5298 &          # then
//   CAIRN_DATA_DIR=/tmp/x PROBE_EXIT=150 scripts/ui-probe.sh scripts/probes/m5-flows.js
// Needs the sample files as `window.M5_SAMPLES` = {name: base64}: `scripts/probes/with-samples.sh`
// writes a probe with them prepended. Reports each step; changes only the throw-away data folder.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const invoke = window.__TAURI_INTERNALS__.invoke
  const port = window.MOCK_LLM_PORT ?? 5298

  // 1. A provider and a model pointing at the mock LLM; default and OCR model.
  const providers = await invoke('list_providers')
  let provider = providers.find((p) => p.name === 'Mock LLM')
  if (!provider) provider = await invoke('create_provider', { input: { name: 'Mock LLM', kind: 'openai_compatible', endpoint: `http://127.0.0.1:${port}/v1`, auth: 'none' } })
  const models = await invoke('list_models')
  let model = models.find((m) => m.providerId === provider.id)
  if (!model) model = await invoke('create_model', { input: { providerId: provider.id, name: 'mock-model', label: 'Mock', format: 'chat_completions', vision: true } })
  await invoke('update_settings', { patch: { defaultModelId: model.id, ocrModelId: model.id } })
  const categories = await invoke('list_categories', { includeArchived: false })
  const category = categories.find((p) => p.key === 'OPS') ?? categories.find((p) => p.isGeneral) ?? categories[0]
  report(`setup: model ${model.label}; category ${category.name}`)

  // 2. A file into a new conversation: upload in chunks, then the file-draft job.
  const upload = async (name) => {
    const b64 = window.M5_SAMPLES[name]
    const size = Math.floor((b64.length * 3) / 4) - (b64.endsWith('==') ? 2 : b64.endsWith('=') ? 1 : 0)
    const id = await invoke('upload_begin', { fileName: name, size })
    for (let i = 0; i < b64.length; i += 2_000_000) await invoke('upload_chunk', { uploadId: id, dataBase64: b64.slice(i, i + 2_000_000) })
    return id
  }
  const waitCard = async (conversationId, messageId, ms = 90_000) => {
    const end = Date.now() + ms
    while (Date.now() < end) {
      const msgs = await invoke('list_messages', { conversationId })
      const m = msgs.find((x) => x.id === messageId)
      if (m && (m.draft || m.error || (m.text && !m.steps.some((s) => s.state === 'active' || s.state === 'pending')))) return m
      await sleep(700)
    }
    return null
  }
  const show = (label, m) => {
    if (!m) return report(`${label}: TIMEOUT`)
    const d = m.draft
    report(`${label}: steps [${m.steps.map((s) => `${s.label} (${s.state})`).join('; ')}]` +
      (d ? ` · card "${d.title}" asIs=${d.asIs} incomplete=${d.incomplete} group=${d.groupId ? 'existing' : d.groupName ?? '-'} tags=${JSON.stringify(d.tags)} terms=${JSON.stringify(d.terms.map((t) => t.term))} topics=${JSON.stringify(d.topics)} bodyStart=${JSON.stringify(d.body.slice(0, 60))}` : '') +
      (m.error ? ` · ERROR ${m.error.kind}: ${m.error.message} canUseAsIs=${m.error.canUseAsIs}` : '') +
      (!d && !m.error ? ` · text ${JSON.stringify(m.text.slice(0, 80))}` : ''))
  }
  const fileDraft = async (name, instruction = '') => {
    const conv = await invoke('create_conversation', { input: { title: 'New page from a file' } })
    const uploadId = await upload(name)
    const started = await invoke('start_file_draft', { input: { conversationId: conv.id, uploadId, instruction, modelId: model.id, categoryId: category.id } })
    const t0 = Date.now()
    const m = await waitCard(conv.id, started.assistantMessageId)
    show(`${name} (${((Date.now() - t0) / 1000).toFixed(1)} s)`, m)
    return { conv, m }
  }
  await fileDraft('one-topic.md')
  const pdf = await fileDraft('pump-manual.pdf')
  await fileDraft('site-visit-scan.pdf')
  await fileDraft('handover-scan.jpg')
  await fileDraft('pre-start-checks.docx')

  // 3. Revising by chatting: a page reply becomes a new card, a question gets a text answer.
  if (pdf.m?.draft) {
    const r1 = await invoke('revise_draft', { input: { conversationId: pdf.conv.id, text: 'Change the title to "EP-40 calibration"', modelId: model.id } })
    show('revise -> page', await waitCard(pdf.conv.id, r1.assistantMessageId))
    const r2 = await invoke('revise_draft', { input: { conversationId: pdf.conv.id, text: 'What K-factor does the manual give?', modelId: model.id } })
    show('revise -> question', await waitCard(pdf.conv.id, r2.assistantMessageId))
    const all = await invoke('list_messages', { conversationId: pdf.conv.id })
    report('cards in the conversation: ' + all.filter((x) => x.draft).map((x) => `"${x.draft.title}"${x.draft.superseded ? ' (earlier)' : ''}`).join(', '))
    window.M5_CONVERSATION = pdf.conv.id
  }

  // 4. The save checks on a draft, and the duplicate check by meaning.
  const body = pdf.m?.draft?.body ?? '# EP-40 pump\n\nCalibrate every 250 hours.'
  try {
    const c = await invoke('check_page', { input: { categoryId: category.id, title: 'EP-40 pump', body, shownTerms: [] } })
    report(`check_page: topics ${JSON.stringify(c.topics)} oneTopic=${c.oneTopic} group=${JSON.stringify(c.group)} tags=${JSON.stringify(c.tags)} terms=${JSON.stringify(c.terms.map((t) => t.term))}`)
  } catch (e) { report('check_page ERROR ' + JSON.stringify(e)) }
  const pages = await invoke('list_pages', { filter: { includeArchived: false } })
  const vpn = pages.find((p) => /vpn/i.test(p.title)) ?? pages[0]
  const vpnPage = await invoke('get_page', { id: vpn.id })
  const dupSelf = await invoke('find_duplicates', { input: { categoryId: vpn.categoryId, title: vpnPage.title, body: vpnPage.body.replace(/\bthe\b/g, 'a') } })
  report(`find_duplicates (reworded "${vpn.title}"): ${dupSelf.meaning.state} ${dupSelf.meaning.itemsReady}/${dupSelf.meaning.items} · ` + dupSelf.hits.map((h) => `${h.title} ${h.score.toFixed(2)}${h.likely ? ' LIKELY' : ''}`).join(' | '))
  const dupNew = await invoke('find_duplicates', { input: { categoryId: category.id, title: 'EP-40 pump', body } })
  report('find_duplicates (pump draft): ' + dupNew.hits.map((h) => `${h.title} ${h.score.toFixed(2)}${h.likely ? ' LIKELY' : ''}`).join(' | '))

  // 5. Task -> knowledge, keep one topic, merge help, suggest groups.
  const tasks = await invoke('list_tasks', { filter: {} })
  const task = tasks[0]
  const comments = await invoke('list_task_comments', { taskId: task.id })
  const td = await invoke('draft_from_task', { input: { taskId: task.id, includeDescription: true, commentIds: comments.map((c) => c.id) } })
  report(`draft_from_task ${task.key}: "${td.title}" incomplete=${td.incomplete}`)
  const k1 = await invoke('keep_one_topic', { input: { categoryId: category.id, body: '# Notes\n\n## Calibration\nx\n\n## Travel budget\ny', topic: 'Calibration', topics: ['Calibration', 'Travel budget'] } })
  report(`keep_one_topic: "${k1.title}"`)
  const sm = await invoke('suggest_merge', { input: { pageId: vpn.id, newBody: '# VPN again\n\nUse the Perth gateway.' } })
  report(`suggest_merge: title "${sm.title}" (must be the existing "${vpnPage.title}") · has conflict line ${/\*\*Conflict:\*\*/.test(sm.body)}`)
  const mb = await invoke('match_merge_blocks', { input: { existing: ['Torque the bolts to 210 Nm.', 'Grease weekly.'], incoming: ['Torque the bolts to 210 Nm.', 'Torque the bolts to 230 Nm.', 'Book flights two weeks ahead.'] } })
  report(`match_merge_blocks byMeaning=${mb.byMeaning}: ` + mb.matches.map((m) => `${m.action}${m.target ?? ''} ${m.score.toFixed(2)}`).join(', '))
  const withUngrouped = categories.find((p) => pages.some((pg) => pg.categoryId === p.id && !pg.groupId)) ?? category
  const sg = await invoke('suggest_groups', { categoryId: withUngrouped.id, jobId: null })
  report(`suggest_groups ${withUngrouped.name}: ${sg.proposals.length} proposals, new groups ${JSON.stringify(sg.newGroups.map((g) => g.name))}`)

  // 6. Without a default model the checks are unavailable (FR-KN-SV-3).
  await invoke('update_settings', { patch: { defaultModelId: '' } })
  try {
    await invoke('check_page', { input: { categoryId: category.id, title: 'x', body: '# x' } })
    report('no model: check_page unexpectedly succeeded')
  } catch (e) { report(`no model: check_page -> ${e.kind}: ${e.message}`) }
  await invoke('update_settings', { patch: { defaultModelId: model.id } })
  report('DONE')
})().catch((err) => report('ERROR ' + (err?.message ?? JSON.stringify(err))))
