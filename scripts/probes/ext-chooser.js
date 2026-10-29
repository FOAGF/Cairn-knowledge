// D-72 probe, scenario A: the full-screen mode chooser on a fresh data folder (FR-KN-EXT-7). Run
// without a preset mode, with the root to choose prepended (a copy of the sample folder):
//   ROOT=/tmp/x-notes; cp -r crates/cairn-core/tests/fixtures/external-sample "$ROOT"
//   { echo "const ROOT = '$ROOT'; const USE_SAMPLE = false"; cat scripts/probes/ext-chooser.js; } > /tmp/p.js
//   CAIRN_DATA_DIR=/tmp/x CAIRN_DEV_KNOWLEDGE_MODE=none PROBE_AFTER=6 PROBE_EXIT=40 \
//     PROBE_DRIVER=scripts/probes/ext-chooser-driver.sh SHOTS=/tmp/shots scripts/probes/ext-probe.sh /tmp/p.js
// USE_SAMPLE = true takes *Use the sample folder (development)* instead of the picker.
// Checks: only the chooser shows (no top bar), a route change and Escape don't get past it, the
// button stays locked until a choice (and, for external, a folder) is made; then external with the
// folder indexes it. Asks the driver for a screenshot with `@@shot-chooser`.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(900)
}
const state = () => `chooser=${!!$('mode-chooser')}, top bar=${!!document.querySelector('header nav')}, continue="${text($('mode-continue'))}" disabled=${$('mode-continue')?.disabled}`

;(async () => {
  try {
    report('A1 start: ' + state() + ' | settings.knowledgeMode=' + (await invoke('get_settings')).knowledgeMode)
    await go('/tasks')
    report('A2 after route /tasks: ' + state() + ' url=' + location.pathname)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await sleep(400)
    report('A3 after Escape: ' + state())
    report('@@shot-chooser')
    await sleep(2500)

    document.querySelector('[data-testid="mode-external"] button[role="radio"]').click()
    await sleep(300)
    report('A4 external ticked, no folder: ' + state() + ', root box="' + text($('mode-root')) + '"')
    if (USE_SAMPLE) {
      $('mode-sample-folder').click()
    } else {
      window.__cairnPickFolder = () => ROOT
      $('mode-pick-folder').click()
    }
    await sleep(800)
    report('A5 folder chosen: ' + state() + ', root box="' + text($('mode-root')) + '"')
    report('@@shot-chooser-ready')
    await sleep(2000)
    $('mode-continue').click()
    const t0 = performance.now()
    let pages = []
    for (let i = 0; i < 60; i++) {
      await sleep(500)
      pages = await invoke('list_pages', { filter: { includeArchived: true } }).catch(() => [])
      if (pages.length >= 7) break
    }
    report(`A6 after continue (${Math.round(performance.now() - t0)} ms): ` + state())
    report('A6 storage: ' + JSON.stringify(await invoke('get_knowledge_storage')))
    report('A6 pages (' + pages.length + '): ' + pages.map((p) => `${p.path}[${p.origin},${p.checkStatus}${p.archived ? ',archived' : ''}]`).sort().join(' | '))
    const cats = await invoke('list_categories', { includeArchived: false })
    report('A6 categories: ' + cats.map((c) => c.name + (c.folder ? '→' + c.folder : '')).join(', '))
    await go('/knowledge')
    await sleep(1500)
    report('A7 /knowledge: top bar=' + !!document.querySelector('header nav') + ', tree text: ' + text(document.querySelector('aside')).slice(0, 400))
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
