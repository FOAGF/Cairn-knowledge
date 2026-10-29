// ui-check.js (M8, D-59): the scripted UI check that check-windows.ps1 runs inside Cairn's window on Windows.
// The app runs it (CAIRN_AUTOMATION=1, CAIRN_DEV_SCRIPT_FILE, CAIRN_DEV_SCRIPT_AFTER=load) on a throw-away
// data folder and takes each line through the `automation_report` command into CAIRN_AUTOMATION_OUT, because
// the window's security policy allows no `fetch`. Every step is one line: `PASS <step> (<ms> ms)` or
// `FAIL <step>: <why> (<ms> ms)`; then `INFO ...` lines, and `DONE` last.
//
// It also runs against a Linux release build (the same commands): CAIRN_AUTOMATION=1 CAIRN_DATA_DIR=<empty folder>.
;(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const report = (line) => invoke('automation_report', { line: String(line) })
  const msg = (e) => (e && e.message ? e.message : typeof e === 'string' ? e : JSON.stringify(e))

  // Security-policy violations, from the very start (expected: none).
  const violations = []
  document.addEventListener('securitypolicyviolation', (ev) => {
    violations.push(`${ev.violatedDirective} blocked ${ev.blockedURI || 'inline'}`)
  })

  let failures = 0
  const step = async (name, fn) => {
    const t0 = performance.now()
    try {
      const note = await fn()
      await report(`PASS ${name} (${Math.round(performance.now() - t0)} ms)${note ? ' ' + note : ''}`)
      return true
    } catch (e) {
      failures++
      await report(`FAIL ${name}: ${msg(e)} (${Math.round(performance.now() - t0)} ms)`)
      return false
    }
  }
  const waitFor = async (what, fn, ms) => {
    for (const began = Date.now(); Date.now() - began < ms; await sleep(50)) {
      const v = fn()
      if (v) return v
    }
    throw new Error('timed out waiting for ' + what)
  }

  try {
    await report(`INFO window ${window.innerWidth}x${window.innerHeight}, ${navigator.userAgent}`)

    // 1. Home shows the counts line (FR-HOME-2), e.g. "0 pages", on the empty data folder.
    await step('home shows the page count', async () => {
      const m = await waitFor('the counts line on Home', () => /([\d,]+)\s+pages?\b/.exec(document.body ? document.body.innerText : ''), 120_000)
      return `(${m[0]})`
    })

    let task = null
    let page = null
    let category = null
    await step('the data folder answers (categories)', async () => {
      const info = await invoke('app_info')
      let categories = await invoke('list_categories', { includeArchived: false })
      if (!categories.length) {
        await invoke('create_category', { input: { name: 'Windows check', key: 'WIN', description: '', color: 'blue' } })
        categories = await invoke('list_categories', { includeArchived: false })
      }
      category = categories[0]
      if (!category) throw new Error('no category')
      return `(Cairn ${info.version}, ${categories.length} category)`
    })

    await step('create a task', async () => {
      if (!category) throw new Error('no category')
      task = await invoke('create_task', { input: { categoryId: category.id, title: 'Windowscheck pump inspection', description: 'Created by the scripted check.' } })
      if (!task || !task.id) throw new Error('no task came back')
      return `(${task.key || task.id})`
    })

    await step('create a page with a misspelt word', async () => {
      if (!category) throw new Error('no category')
      page = await invoke('create_page', {
        input: { categoryId: category.id, title: 'Windowscheck notes', body: 'Teh quick brown fox jumpd over the lazy dog. Windowscheck page.' },
      })
      if (!page || !page.id) throw new Error('no page came back')
    })

    await step('keyword search finds the task', async () => {
      const hits = await invoke('search', { query: { text: 'Windowscheck' } })
      if (!task || !hits.some((h) => h.id === task.id)) throw new Error(`task not among ${hits.length} hits`)
      if (!page || !hits.some((h) => h.id === page.id)) throw new Error(`page not among ${hits.length} hits`)
      return `(${hits.length} hits)`
    })

    await step('the spell checker answers (lint_text)', async () => {
      const hits = await invoke('lint_text', { text: 'Teh quick brown fox jumpd over teh lazy dog.', options: { spelling: true, grammar: true } })
      if (!hits.length) throw new Error('no mistakes found in a sentence full of them')
      return `(${hits.length} hits)`
    })

    await step('open the page and see a spelling underline', async () => {
      if (!page) throw new Error('no page')
      history.pushState({}, '', '/knowledge/' + page.id)
      window.dispatchEvent(new PopStateEvent('popstate'))
      const edit = await waitFor('the Edit button', () => [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Edit'), 30_000)
      edit.click()
      const cm = await waitFor('the editor', () => document.querySelector('.cm-content'), 30_000)
      // Nudge the editor with one more misspelt word, so its check runs after the editor is up.
      const view = (cm.cmTile && cm.cmTile.root && cm.cmTile.root.view) || (cm.cmView && cm.cmView.view)
      if (view) view.dispatch({ changes: { from: view.state.doc.length, insert: ' Teh end.' } })
      // The first check loads Harper: allow it time, particularly on the first start of a scanned download.
      const n = await waitFor('a spelling underline', () => document.querySelectorAll('.cm-lintRange-error, [class*="cm-lintRange"]').length, 60_000)
      return `(${n} underline${n === 1 ? '' : 's'})`
    })

    await step('no security-policy violations', async () => {
      await sleep(300)
      if (violations.length) throw new Error(`${violations.length}: ${violations.slice(0, 3).join(' | ')}`)
      return '(0)'
    })
  } catch (e) {
    failures++
    await report(`FAIL the check itself: ${msg(e)}`)
  }

  await report(`INFO ${failures ? failures + ' step(s) failed' : 'all steps passed'}`)
  await report('DONE')
  // Close the window (the main window may be destroyed: capability core:window:allow-destroy). If this is
  // refused, CAIRN_EXIT_AFTER_SECS ends the app.
  try {
    await invoke('plugin:window|destroy', { label: 'main' })
  } catch (e) {
    await report(`INFO could not close the window: ${msg(e)}`)
  }
})()
