// The script `make measure` runs inside Cairn's window (NFR-PERF-6, D-51). scripts/measure/measure.py
// puts a `CFG` object in front of it and starts the app with CAIRN_AUTOMATION=1, so the app runs it
// (CAIRN_DEV_SCRIPT_FILE, started when the page has loaded) and takes its reports through the
// `automation_report` command into CAIRN_AUTOMATION_OUT. The app's content security policy allows no
// `fetch` to a local listener, so there is no other way out of the window.
//
// One app start measures one thing (CFG.mode), so each state's memory is that state alone:
//   idle       start-up to a usable window, memory at idle, CPU while idle, keyword search times
//   meaning    a search by meaning (the embedding model loads), hybrid search times
//   editing    a page open for editing with spelling and grammar checks running
//   assistant  one Assistant answer from the mock LLM
// Lines: `STARTUP ...`, `STATE name` + `STATE_END name` around each hold (the helper samples memory
// in between), `TIMES kind {json}`, `INFO ...`, `ERROR ...`, and `DONE` last.
;(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const report = (line) => invoke('automation_report', { line })
  const fail = async (what, e) => {
    await report(`ERROR ${what}: ${e && e.message ? e.message : JSON.stringify(e)}`)
    await report('DONE')
  }
  const hold = async (name, ms) => {
    await report(`STATE ${name}`)
    await sleep(ms)
    await report(`STATE_END ${name}`)
  }
  const stats = (xs) => {
    const s = [...xs].sort((a, b) => a - b)
    const at = (p) => s[Math.min(s.length - 1, Math.ceil(p * s.length) - 1)]
    return { n: s.length, median: at(0.5), p95: at(0.95), max: s[s.length - 1] }
  }
  const timeQueries = async (command, queries, rounds) => {
    // One discarded round first, so file caches and the first query's one-off costs are out of it.
    const times = []
    for (let round = 0; round <= rounds; round++) {
      for (const text of queries) {
        const t0 = performance.now()
        await invoke(command, { query: { text } })
        if (round > 0) times.push(performance.now() - t0)
      }
    }
    return times
  }

  try {
    // Start-up: the moment Home shows the page count of the data and its Today list. The
    // helper takes the time from its own start of the app to this moment (same clock, Date.now()).
    let usable = null
    // Home's counts line under the greeting, e.g. "10000 pages" (FR-HOME-2).
    const PAGE_COUNT = /([\d,]+)\s+pages?\b/
    const text = () => (document.body ? document.body.innerText : '')
    const lastSeen = () => {
      const m = PAGE_COUNT.exec(text())
      return m ? m[1] : '-'
    }
    // A task key in its own element: in the page text the key runs straight into the title
    // ("PMP-12Check the pump"), so a pattern with a word boundary misses it.
    const taskKeyShown = () =>
      [...document.querySelectorAll('li span.font-mono')].some((s) => /^[A-Z][A-Z0-9]*-\d+$/.test(s.textContent.trim()))
    for (const began = Date.now(); Date.now() - began < 90_000; ) {
      const t = text()
      const m = PAGE_COUNT.exec(t)
      if (m && Number(m[1].replace(/,/g, '')) === CFG.expectPages && taskKeyShown()) {
        usable = Date.now()
        break
      }
      await sleep(20)
    }
    if (usable === null)
      return fail('startup', {
        message: `Home never showed ${CFG.expectPages} pages and an active task (pages seen: ${lastSeen()}, task key seen: ${taskKeyShown() ? 'yes' : 'no'})`,
      })
    await report(`STARTUP usableAt=${usable} timeOrigin=${Math.round(performance.timeOrigin)}`)
    await sleep(3000) // let the first renders and the indexer's first pass settle

    if (CFG.mode === 'idle') {
      const info = await invoke('app_info')
      await report(`INFO version ${info.version}`)
      const progress = await invoke('get_index_progress')
      const st = await invoke('sidecar_status')
      await report(`INFO index ${progress.status.chunksReady}/${progress.status.chunks} pieces ready, model ${progress.status.model} dims ${progress.status.dims}, sidecars ${JSON.stringify(st)}`)
      await hold('idle', CFG.holdMs)
      await hold('cpu', CFG.cpuSecs * 1000 + 2000)
      const times = await timeQueries('search', CFG.keywordQueries, CFG.rounds)
      await report(`TIMES keyword ${JSON.stringify({ ...stats(times), queries: CFG.keywordQueries.length, rounds: CFG.rounds })}`)
    } else if (CFG.mode === 'meaning') {
      // The first search by meaning starts the search model; its time is the model's load.
      const t0 = performance.now()
      const first = await invoke('search_hybrid', { query: { text: CFG.hybridQueries[0] } })
      await report(`INFO first hybrid search (loads the model) ${Math.round(performance.now() - t0)} ms, meaning ${first.meaning.state}${first.meaning.detail ? ' ' + first.meaning.detail : ''}`)
      if (first.meaning.state === 'unavailable') return fail('meaning', { message: first.meaning.detail || 'the search model is unavailable' })
      for (const text of CFG.hybridQueries.slice(1, 4)) await invoke('search_hybrid', { query: { text } })
      await hold('meaning', CFG.holdMs)
      const times = await timeQueries('search_hybrid', CFG.hybridQueries, CFG.rounds)
      await report(`TIMES hybrid ${JSON.stringify({ ...stats(times), queries: CFG.hybridQueries.length, rounds: CFG.rounds })}`)
    } else if (CFG.mode === 'editing') {
      const hits = await invoke('search', { query: { text: 'pump', kinds: ['page'] } })
      if (!hits.length) return fail('editing', { message: 'no page to open' })
      history.pushState({}, '', '/knowledge/' + hits[0].id)
      window.dispatchEvent(new PopStateEvent('popstate'))
      const find = async (what, fn) => {
        for (let i = 0; i < 300; i++) {
          const el = fn()
          if (el) return el
          await sleep(50)
        }
        throw new Error('not found: ' + what)
      }
      const edit = await find('Edit button', () => [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Edit'))
      edit.click()
      const cm = await find('editor', () => document.querySelector('.cm-content'))
      // Typed text with mistakes. Through the editor's own view when it can be reached (the window
      // may not have the keyboard focus, which `execCommand` needs), else as a typed insert.
      const typed = '\n\nTeh quick brown fox jumpd over teh lazy dog and and then it went home.'
      const view = (cm.cmTile && cm.cmTile.root && cm.cmTile.root.view) || (cm.cmView && cm.cmView.view)
      if (view) view.dispatch({ changes: { from: view.state.doc.length, insert: typed } })
      else {
        cm.focus()
        document.execCommand('insertText', false, typed)
      }
      // The editor checks 0.5 s after the last key; wait for its underlines (the first check loads Harper; slow in a debug build).
      let ranges = 0
      for (let i = 0; i < 400 && !ranges; i++) {
        await sleep(100)
        ranges = document.querySelectorAll('[class*="cm-lintRange"]').length
      }
      // The same call without the editor, so Harper is surely loaded even if the editor didn't ask.
      const hitsLint = await invoke('lint_text', { text: 'Teh quick brown fox jumpd over teh lazy dog.', options: { spelling: true, grammar: true } })
      await report(`INFO editor underlines ${ranges} (${view ? 'dispatched' : 'typed'}; ${view ? view.state.doc.length : cm.textContent.length} characters; lint classes ${[...new Set([...document.querySelectorAll('[class*="cm-lint"]')].map((e) => e.className))].join(' | ')}), lint_text hits ${hitsLint.length}`)
      await hold('editing', CFG.holdMs)
    } else if (CFG.mode === 'assistant') {
      const settings = await invoke('get_settings')
      if (!settings.defaultModelId) return fail('assistant', { message: 'no default model in the data' })
      const conv = await invoke('create_conversation', { input: { title: '' } })
      const started = await invoke('send_message', {
        input: { conversationId: conv.id, text: CFG.assistantQuestion, modelId: settings.defaultModelId, effort: null, uploadId: null, web: false },
      })
      const t0 = performance.now()
      let done = null
      for (let i = 0; i < 360 && !done; i++) {
        const m = (await invoke('list_messages', { conversationId: conv.id })).find((x) => x.id === started.assistantMessageId)
        const running = m && m.steps.some((s) => s.state === 'active' || s.state === 'pending')
        if (m && !running && (m.answer || m.find || m.proposal || m.draft || m.error || m.text)) done = m
        else await sleep(250)
      }
      if (!done) return fail('assistant', { message: 'no answer within 90 s' })
      await report(`INFO assistant answered in ${Math.round(performance.now() - t0)} ms${done.error ? ', ERROR ' + done.error.kind + ': ' + done.error.message : ', ' + (done.text || '').length + ' chars'}`)
      await hold('assistant', CFG.holdMs)
    }
    await report('DONE')
  } catch (e) {
    await fail(CFG.mode, e)
  }
})()
