// M6 assistant flows end to end in the real app, on the dev sample data, with the real sidecar (search
// model, file extraction) and the mock LLM (`scripts/mock_llm.py`, port MOCK_LLM_PORT, default 5298):
//   python3 -I scripts/mock_llm.py 5298 &          # then
//   CAIRN_DATA_DIR=/tmp/x PROBE_EXIT=240 scripts/ui-probe.sh "$(scripts/probes/with-samples.sh scripts/probes/m6-flows.js)"
// The sample files (`window.M5_SAMPLES`) are only needed for the file question. Reports each step; changes
// only the throw-away data folder.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const invoke = window.__TAURI_INTERNALS__.invoke
  const port = window.MOCK_LLM_PORT ?? 5298
  const short = (s, n = 90) => JSON.stringify((s ?? '').slice(0, n))

  // 1. A provider and a model pointing at the mock LLM, with Web search ticked; the default model.
  const providers = await invoke('list_providers')
  let provider = providers.find((p) => p.name === 'Mock LLM')
  if (!provider) provider = await invoke('create_provider', { input: { name: 'Mock LLM', kind: 'openai_compatible', endpoint: `http://127.0.0.1:${port}/v1`, auth: 'none' } })
  const models = await invoke('list_models')
  let model = models.find((m) => m.providerId === provider.id)
  if (!model) model = await invoke('create_model', { input: { providerId: provider.id, name: 'mock-model', label: 'Mock', format: 'chat_completions', vision: true, webSearch: true } })
  else model = await invoke('update_model', { id: model.id, patch: { webSearch: true } })
  await invoke('update_settings', { patch: { defaultModelId: model.id, ocrModelId: model.id } })
  const categories = await invoke('list_categories', { includeArchived: false })
  report(`setup: model ${model.label} webSearch=${model.webSearch}; categories ${categories.map((p) => p.key).join(', ')}`)

  // 2. Wait for the search index (the real search model) so answers have sources by meaning.
  for (let i = 0; i < 90; i++) {
    const p = await invoke('get_index_progress')
    if (p.status.items > 0 && p.status.itemsReady === p.status.items && !p.running) {
      report(`index ready: ${p.status.itemsReady}/${p.status.items} items`)
      break
    }
    if (i % 10 === 0) report(`indexing ${p.status.itemsReady}/${p.status.items}${p.error ? ' error ' + p.error : ''}`)
    await sleep(2000)
  }

  const waitDone = async (conversationId, messageId, ms = 60_000) => {
    const end = Date.now() + ms
    while (Date.now() < end) {
      const m = (await invoke('list_messages', { conversationId })).find((x) => x.id === messageId)
      const running = m && m.steps.some((s) => s.state === 'active' || s.state === 'pending')
      if (m && !running && (m.answer || m.find || m.proposal || m.draft || m.error || m.text)) return m
      await sleep(500)
    }
    return null
  }
  const show = (label, m) => {
    if (!m) return report(`${label}: TIMEOUT`)
    const a = m.answer, f = m.find, p = m.proposal
    report(`${label}: steps [${m.steps.map((s) => `${s.label} (${s.state})`).join('; ')}]` +
      ` · text ${short(m.text)}` +
      (a ? ` · answer basis=${a.basis} sources=${a.sources.length} cited=${JSON.stringify(a.cited)} more=${a.more.length} outside=${JSON.stringify(a.outsideScope)} terms=${JSON.stringify(a.terms.map((t) => t.term))} unticked=${short(a.webUnticked, 60)} first=${a.sources[0] ? `${a.sources[0].kind}:${short(a.sources[0].title, 40)} part=${short(a.sources[0].part, 30)} url=${a.sources[0].url ?? '-'}` : '-'}` : '') +
      (f ? ` · find intro=${short(f.intro)} items=${f.items.length} [${f.items.slice(0, 4).map((x) => x.key ?? x.title).join(', ')}]` : '') +
      (p ? ` · proposal rows=${p.rows.length} [${p.rows.map((r) => `${r.change.op}:${r.state}${r.ticked ? '+' : '-'}`).join(', ')}] terms=${JSON.stringify(p.terms.map((t) => t.term))}` : '') +
      (m.draft ? ` · draft "${m.draft.title}" group=${m.draft.groupId ? 'existing' : m.draft.groupName ?? '-'} tags=${JSON.stringify(m.draft.tags)}` : '') +
      (m.error ? ` · ERROR ${m.error.kind}: ${m.error.message}` : ''))
  }
  const send = async (conversationId, text, extra = {}) => {
    const started = await invoke('send_message', { input: { conversationId, text, modelId: model.id, effort: null, uploadId: null, web: false, ...extra } })
    return waitDone(conversationId, started.assistantMessageId)
  }

  // 3. Questions.
  const conv = await invoke('create_conversation', { input: { title: '' } })
  const q1 = await send(conv.id, 'How do I calibrate the EP-40 flow meter?')
  show('question', q1)
  report(`  conversation title now ${short((await invoke('list_conversations')).find((c) => c.id === conv.id)?.title)}`)
  show('general', await send(conv.id, 'What is the boiling point of water? general knowledge'))
  show('follow-up (second one)', await send(conv.id, 'tell me more about the second one'))
  show('find', await send(conv.id, 'List my blocked tasks'))

  // 4. Scope: a conversation limited to the first category, a question naming another one.
  const [inside, outsideP] = [categories[0], categories.find((p) => p.id !== categories[0].id)]
  const scoped = await invoke('create_conversation', { input: { title: 'Scoped', scopeCategoryIds: [inside.id] } })
  const os = await send(scoped.id, `What do the ${outsideP.name} notes say about setup?`)
  show(`outside scope (${outsideP.name})`, os)
  if (os?.answer?.outsideScope?.length) {
    await invoke('search_all_categories', { messageId: os.id })
    await sleep(500)
    show('search all categories', await waitDone(scoped.id, os.id))
  }

  // 5. Actions: create a task and confirm; change tasks; a new page; a page change; delete is refused.
  const acts = await invoke('create_conversation', { input: { title: 'Actions', scopeCategoryIds: [inside.id] } })
  const c1 = await send(acts.id, 'Create a task to order a spare hose for the truck')
  show('create task', c1)
  if (c1?.proposal) {
    const done = await invoke('confirm_proposal', { messageId: c1.id })
    report(`  confirm: ${done.proposal.rows.map((r) => `${r.state} ${r.result ?? ''}`).join(' | ')}`)
  }
  const c2 = await send(acts.id, 'Mark the hose task as done')
  show('change tasks', c2)
  if (c2?.proposal && c2.proposal.rows.length > 1) {
    await invoke('set_proposal_row', { messageId: c2.id, row: 1, ticked: false })
    const done = await invoke('confirm_proposal', { messageId: c2.id })
    report(`  confirm with row 2 unticked: ${done.proposal.rows.map((r) => `${r.state} ${r.result ?? ''}`).join(' | ')}`)
  }
  show('new page', await send(acts.id, 'Make a page about what we know on the pump'))
  const pe = await send(acts.id, 'Update the calibration page with a checked note')
  show('page change', pe)
  if (pe?.proposal?.rows.some((r) => r.change.op === 'update_page')) {
    const row = pe.proposal.rows.find((r) => r.change.op === 'update_page')
    report(`  diff: old ${row.change.oldBody.length} chars -> new ${row.change.newBody.length} chars, ends ${short(row.change.newBody.slice(-40))}`)
  }
  show('delete', await send(acts.id, 'Delete the hose task'))

  // 6. Web search: a cited answer; an ignored request unticks the model (Chat Completions).
  const web = await invoke('create_conversation', { input: { title: 'Web' } })
  show('web (searched)', await send(web.id, 'What is the latest Python release?', { web: true }))
  const user = (await invoke('list_messages', { conversationId: web.id })).find((m) => m.role === 'user')
  report(`  user message extra ${user?.extra}`)
  show('web (ignored)', await send(web.id, 'What is the latest Python release? ignore', { web: true }))
  const after = (await invoke('list_models')).find((m) => m.id === model.id)
  report(`  model webSearch after the ignored request: ${after.webSearch}`)
  await invoke('update_model', { id: model.id, patch: { webSearch: true } })

  // 7. History budget.
  const u = await invoke('history_usage', { conversationId: conv.id, modelId: model.id })
  report(`history: used ${u.used} of ${u.budget} (model limit ${u.modelLimit}) -> ${u.percentLeft}% left, canWiden=${u.canWiden}`)
  await invoke('update_settings', { patch: { assistantHistoryBudget: 1000 } })
  const small = await invoke('history_usage', { conversationId: conv.id, modelId: model.id })
  report(`history at a 1000-token budget: used ${small.used}, ${small.percentLeft}% left, canWiden=${small.canWiden}`)
  if (small.canWiden) {
    const w = await invoke('widen_history', { conversationId: conv.id, modelId: model.id })
    report(`  widened: budget ${w.budget}, ${w.percentLeft}% left`)
  }
  await invoke('update_settings', { patch: { assistantHistoryBudget: 30000 } })

  // 8. A question about an attached file (and the document's glossary suggestions).
  if (window.M5_SAMPLES?.['pump-manual.pdf']) {
    const b64 = window.M5_SAMPLES['pump-manual.pdf']
    const size = Math.floor((b64.length * 3) / 4) - (b64.endsWith('==') ? 2 : b64.endsWith('=') ? 1 : 0)
    const id = await invoke('upload_begin', { fileName: 'pump-manual.pdf', size })
    await invoke('upload_chunk', { uploadId: id, dataBase64: b64 })
    const fq = await invoke('create_conversation', { input: { title: '' } })
    show('file question', await send(fq.id, 'What torque does this give for the mounting bolts?', { uploadId: id }))
  }

  // 9. Without a default model and with the conversation's model unset, the job reports what to set up.
  report('DONE')
})().catch((err) => report('ERROR ' + (err?.message ?? JSON.stringify(err))))
