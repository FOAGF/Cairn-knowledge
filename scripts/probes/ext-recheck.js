// D-72 recheck probe, external mode on a copy of the sample folder, for the fixes after the first
// real-app check. Run with its driver (ext-recheck-driver.sh), which edits files and takes shots:
//   CAIRN_DATA_DIR=/tmp/x CAIRN_DEV_KNOWLEDGE_MODE=external=$ROOT CAIRN_DEV_TRASH_DIR=/tmp/t \
//     ROOT=$ROOT SHOTS=/tmp/shots PROBE_AFTER=1 PROBE_EXIT=120 \
//     PROBE_DRIVER=scripts/probes/ext-recheck-driver.sh scripts/probes/ext-probe.sh scripts/probes/ext-recheck.js
// R0 one "Learning" category (dev seed vs. the first scan), R7 the watcher error (card alert, one
// toast, *Rescan* picks up an outside edit), R3 the conflict diff's link forms, R1 a rename to
// `.Pump: check?` (file name, `# title` line, one H1), R1b a new page with such a title through the
// review screen, R6 the case-clash message.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1200)
}
const button = (t) => [...document.querySelectorAll('button')].find((b) => text(b) === t)
const editing = () => !!document.querySelector('[role="toolbar"][aria-label="Editing"]')
const view = (root = document) => root.querySelector('.cm-content')?.cmTile?.root?.view
const typeAtEnd = (s, v = view()) => v.dispatch({ changes: { from: v.state.doc.length, insert: s }, selection: { anchor: v.state.doc.length + s.length } })
const setInput = (el, v) => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v)
  el.dispatchEvent(new Event('input', { bubbles: true }))
}
const all = async () => invoke('list_pages', { filter: { includeArchived: true } })
const byTitle = async (t) => (await all()).find((p) => p.title === t)
const h1s = () => [...document.querySelectorAll('article h1')].map((h) => '"' + text(h) + '"').join(' + ')
// Every toast text seen, with when it appeared.
const t0 = performance.now()
const toasts = []
new MutationObserver(() => {
  for (const t of document.querySelectorAll('[data-sonner-toast]')) {
    if (!t.dataset.seen) {
      t.dataset.seen = '1'
      toasts.push(`+${Math.round((performance.now() - t0) / 100) / 10}s ${text(t).slice(0, 170)}`)
    }
  }
}).observe(document.body, { childList: true, subtree: true })

;(async () => {
  try {
    let pages = []
    for (let i = 0; i < 40 && pages.length < 7; i++) {
      pages = await all()
      await sleep(500)
    }
    await sleep(6000)
    const cats = await invoke('list_categories', { includeArchived: false })
    report('R0 categories: ' + cats.map((c) => c.name + (c.folder ? '→' + c.folder : '')).join(', ') + ' | "Learning" count=' + cats.filter((c) => c.name === 'Learning').length)

    // R7: the watcher can't start here.
    report('R7 storage: ' + JSON.stringify(await invoke('get_knowledge_storage')))
    await go('/settings/general')
    await sleep(800)
    report('R7 alert: ' + text($('knowledge-watch-error')))
    report('R7 toasts so far: ' + toasts.join(' | '))
    $('knowledge-storage-card')?.scrollIntoView({ block: 'start' })
    window.scrollBy(0, -80)
    await sleep(500)
    report('@@shot-card')
    await sleep(2500)
    report('@@outside-welcome')
    await sleep(3000)
    const welcome = await byTitle('Welcome')
    report('R7 3 s after an outside edit, before Rescan: index has it=' + (await invoke('get_page', { id: welcome.id })).body.includes('RESCAN-EDIT'))
    $('storage-rescan').click()
    let picked = -1
    const r0 = performance.now()
    for (let i = 0; i < 100; i++) {
      if ((await invoke('get_page', { id: welcome.id })).body.includes('RESCAN-EDIT')) {
        picked = Math.round(performance.now() - r0)
        break
      }
      await sleep(100)
    }
    await sleep(1500)
    report(`R7 after Rescan: picked up after ${picked} ms; status "${text($('rescan-status'))}"; alert still shown=${!!$('knowledge-watch-error')}`)

    // R3: the conflict diff on Pump check (page links, a [[link]] and an image).
    const pump = await byTitle('Pump check')
    await go('/knowledge/' + pump.id)
    button('Edit').click()
    await sleep(700)
    typeAtEnd('\nMINE typed in Cairn.\n')
    await sleep(300)
    report('@@outside-pump')
    await sleep(1500)
    button('Save').click()
    await sleep(1500)
    report('R3 conflict dialog=' + !!$('save-conflict-dialog'))
    $('conflict-show-diff')?.click()
    await sleep(1500)
    const diff = text($('conflict-diff'))
    report('R3 diff: ' + diff.slice(0, 500))
    report('R3 diff has "page:"=' + diff.includes('page:') + ', has "folder:"=' + diff.includes('folder:') + ', has %20=' + diff.includes('%20'))
    $('conflict-diff')?.querySelector('button')?.click() // "Show the whole page"
    await sleep(800)
    report('R3 whole page: ' + text($('conflict-diff')).slice(0, 600))
    report('@@shot-conflict')
    await sleep(2500)
    $('conflict-take-disk').click()
    await sleep(1500)
    report('R3 after Take the disk version: editing=' + editing() + ', shows OUTSIDE=' + text(document.querySelector('article')).includes('OUTSIDE written'))

    // R1: rename through the page editor.
    button('Edit').click()
    await sleep(700)
    setInput(document.querySelector('input[aria-label="Title"]'), '.Pump: check?')
    await sleep(400)
    report('R1 Saved as: ' + text($('saved-as')))
    button('Save').click()
    await sleep(2500)
    const renamed = await invoke('get_page', { id: pump.id })
    report(`R1 saved: title="${renamed.title}", path=${renamed.path}, body starts ${JSON.stringify(renamed.body.slice(0, 20))}; H1s on screen: ${h1s()}`)
    report('@@dump-pump')
    await sleep(1000)
    report('@@shot-title')
    await sleep(2500)

    // R1b: a new page through the review screen, with a `# title` line.
    await go('/knowledge/new')
    ;[...document.querySelectorAll('h2')].find((h) => text(h) === 'Write manually').closest('button').click()
    await sleep(1500)
    const titleBox = document.querySelector('input[placeholder="Page title"]')
    setInput(titleBox, '.Doc: one?')
    await sleep(300)
    const v = view(document.querySelector('main'))
    v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: '# .Doc: one?\n\nWritten on the review screen.' } })
    await sleep(800)
    report('R1b review: title box "' + titleBox.value + '", ' + text($('saved-as')) + ', editor now ' + JSON.stringify(view(document.querySelector('main'))?.state.doc.toString()))
    button('Save page').click()
    await sleep(3000)
    const doc = (await all()).find((p) => p.title.startsWith('Doc'))
    report(`R1b saved: url=${location.pathname}, title="${doc?.title}", path=${doc?.path}, body ${JSON.stringify((await invoke('get_page', { id: doc.id })).body)}; H1s on screen: ${h1s()}`)
    report('@@dump-doc')
    await sleep(1000)

    // R6: the case clash names the page already there.
    const learning = cats.find((c) => c.folder === 'Learning')
    const r6 = await invoke('create_page', { input: { categoryId: learning.id, title: 'rust OWNERSHIP', body: 'x' } }).then((p) => 'created ' + p.path, (e) => 'refused: ' + (e.message ?? JSON.stringify(e)))
    report('R6 ' + r6)
    report('R7 all toasts in this run: ' + toasts.join(' | '))
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
