// D-72 scale check, scenario F: external mode on a large folder (ext-gen-folder.py). Follows the
// first scan's `knowledge-progress` events from the window's start and, meanwhile, times a command
// round-trip and the gaps between animation frames every 250 ms (the UI stays responsive); then
// times the knowledge tree and a keyword search, and idles so the driver can sample idle CPU.
//   python3 -I scripts/probes/ext-gen-folder.py /tmp/x-big 10000
//   CAIRN_DATA_DIR=/tmp/x CAIRN_DEV_KNOWLEDGE_MODE=external=/tmp/x-big CAIRN_SIDECAR_PYTHON=/nonexistent \
//     CPU=/tmp/cpu.txt PROBE_AFTER=1 PROBE_EXIT=300 PROBE_DRIVER=scripts/probes/ext-scale-driver.sh \
//     scripts/probes/ext-probe.sh scripts/probes/ext-scale.js
// (CAIRN_SIDECAR_PYTHON pointing nowhere keeps the AI worker from embedding 10,000 pages for hours.)
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
}
const listen = (event, cb) =>
  invoke('plugin:event|listen', { event, target: { kind: 'Any' }, handler: window.__TAURI_INTERNALS__.transformCallback(cb) })
const stats = (xs) => {
  const s = [...xs].sort((a, b) => a - b)
  return s.length ? `n=${s.length} median ${Math.round(s[s.length >> 1])} ms, p95 ${Math.round(s[Math.floor(s.length * 0.95)])} ms, max ${Math.round(s[s.length - 1])} ms` : 'n=0'
}

;(async () => {
  try {
    const t0 = performance.now()
    let done = null
    let first = null
    let lastStep = ''
    await listen('knowledge-progress', (e) => {
      const p = e.payload
      if (!first) first = performance.now()
      if (p.step !== lastStep) {
        report(`progress +${Math.round(performance.now() - t0)} ms: ${p.step} ${p.done}/${p.total}`)
        lastStep = p.step
      }
      if (p.finished) done = { t: performance.now(), p }
    })
    report('@@probe-start')
    // Responsiveness while the scan runs: command round-trips and frame gaps.
    const rtt = []
    const rttPages = []
    const frames = []
    let lastFrame = performance.now()
    let watching = true
    const frame = (t) => {
      frames.push(t - lastFrame)
      lastFrame = t
      if (watching) requestAnimationFrame(frame)
    }
    requestAnimationFrame(frame)
    await go('/knowledge')
    while (!done && performance.now() - t0 < 240000) {
      let s = performance.now()
      await invoke('get_settings')
      rtt.push(performance.now() - s)
      s = performance.now()
      await invoke('list_pages', { filter: { limit: 50 } })
      rttPages.push(performance.now() - s)
      await sleep(250)
    }
    watching = false
    report('@@scan-done')
    report(`F1 scan: first event +${Math.round(first - t0)} ms, finished +${Math.round(done?.t - t0)} ms after the probe started; summary ${JSON.stringify(done?.p.scan)?.slice(0, 300)} ${done?.p.error ?? ''}`)
    report(`F2 during the scan: get_settings ${stats(rtt)}; list_pages(50) ${stats(rttPages)}; frame gaps ${stats(frames)}; frames over 100 ms: ${frames.filter((f) => f > 100).length}`)
    // The tree and a search with 10,000 pages.
    await go('/tasks')
    await sleep(1500)
    let s = performance.now()
    await go('/knowledge')
    let shown = -1
    for (let i = 0; i < 600; i++) {
      if (document.querySelectorAll('aside a[href^="/knowledge/"]').length > 100) {
        shown = performance.now() - s
        break
      }
      await sleep(20)
    }
    report(`F3 /knowledge tree with ${document.querySelectorAll('aside a[href^="/knowledge/"]').length} page links shown after ${Math.round(shown)} ms`)
    s = performance.now()
    const all = await invoke('list_pages', { filter: { includeArchived: true } })
    report(`F3 list_pages (all ${all.length}) took ${Math.round(performance.now() - s)} ms`)
    s = performance.now()
    const hits = await invoke('search', { query: { text: 'pump seal', limit: 20 } }).catch((e) => ({ error: e.message ?? JSON.stringify(e) }))
    report(`F3 search "pump seal": ${Math.round(performance.now() - s)} ms → ${JSON.stringify(hits).slice(0, 160)}`)
    report('@@idle-start')
    await sleep(75000)
    report('@@idle-end')
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
