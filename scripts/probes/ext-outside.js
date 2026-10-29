// D-72 probe, external mode: changes made outside Cairn while it runs (FR-KN-EXT-4, -6). Run with
// its driver, which writes the files when the probe asks and samples the app's CPU:
//   CAIRN_DATA_DIR=/tmp/x CAIRN_DEV_KNOWLEDGE_MODE=external=$ROOT ROOT=$ROOT CPU=/tmp/cpu.txt \
//     PROBE_AFTER=10 PROBE_EXIT=150 PROBE_DRIVER=scripts/probes/ext-outside-driver.sh \
//     scripts/probes/ext-probe.sh scripts/probes/ext-outside.js
// B4: an edit, a new file and a removed file show within ~2 s (in the index and on screen).
// B5: 500 new files at once, then 500 edits spread over 5 s: the index catches up, and the
//     `data-changed` events for knowledge (one per watcher pass) come about once a second.
// B6: renames made outside keep the page's id, its task link and its history.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1000)
}
const all = async () => invoke('list_pages', { filter: { includeArchived: true } })
const byTitle = async (t) => (await all()).find((p) => p.title === t)
const article = () => text(document.querySelector('article'))
/** Polls `check` every 50 ms; returns ms until true, or -1 after `limit` ms. */
const until = async (check, limit = 15000) => {
  const t0 = performance.now()
  while (performance.now() - t0 < limit) {
    if (await check()) return Math.round(performance.now() - t0)
    await sleep(50)
  }
  return -1
}
// Every data-changed event, with its time (one per watcher pass that changed something).
const changes = []
const listen = (event, cb) =>
  invoke('plugin:event|listen', { event, target: { kind: 'Any' }, handler: window.__TAURI_INTERNALS__.transformCallback(cb) })
const passesBetween = (a, b) => changes.filter((c) => c.t >= a && c.t <= b && JSON.stringify(c.p).includes('knowledge'))

;(async () => {
  try {
    await listen('data-changed', (e) => changes.push({ t: performance.now(), p: e.payload }))
    let pages = []
    for (let i = 0; i < 40 && pages.length < 7; i++) {
      pages = await all()
      await sleep(500)
    }
    report('start: ' + pages.length + ' pages')

    // B4: an edit while the page is shown.
    const rust = await byTitle('Rust ownership')
    await go('/knowledge/' + rust.id)
    report('@@b4-edit')
    const tIndex = await until(async () => (await invoke('get_page', { id: rust.id })).body.includes('OUTSIDE-EDIT'))
    const tShown = await until(() => article().includes('OUTSIDE-EDIT'))
    report(`B4 edit: in the index after ${tIndex} ms, on screen ${tShown >= 0 ? 'by then +' + tShown + ' ms' : 'never'} (from the request; the driver writes within ~50 ms)`)
    report('@@b4-new')
    const tNew = await until(async () => !!(await byTitle('Made outside')))
    await go('/knowledge')
    report(`B4 new file: in the index after ${tNew} ms; tree shows it=${text(document.querySelector('aside')).includes('Made outside')}`)
    const made = await byTitle('Made outside')
    report(`B4 new page: origin=${made?.origin}, check=${made?.checkStatus}, path=${made?.path}`)
    report('@@b4-remove')
    const tGone = await until(async () => !(await byTitle('Made outside')))
    report(`B4 removed file: gone from the index after ${tGone} ms`)
    report('@@b4-folder')
    const tFolder = await until(async () => (await invoke('list_categories', { includeArchived: false })).some((c) => c.name === 'Projects'))
    const cat = (await invoke('list_categories', { includeArchived: false })).find((c) => c.name === 'Projects')
    report(`B4 new folder: category after ${tFolder} ms: ${cat ? cat.name + ' key ' + cat.key + ' folder ' + cat.folder : 'none'}; pages in it: ${(await all()).filter((p) => p.categoryId === cat?.id).map((p) => p.path).join(', ')}`)

    // B5: a burst of 500 new files, then 500 edits over 5 s.
    const before = (await all()).length
    await sleep(2000)
    const b0 = performance.now()
    report('@@b5-burst')
    const tBurst = await until(async () => (await all()).length >= before + 500, 60000)
    const b1 = performance.now()
    await sleep(3000)
    report(`B5 burst: ${(await all()).length - before} new pages; all 500 in the index after ${tBurst} ms; knowledge passes during: ${passesBetween(b0, b1 + 3000).length}`)
    // A command round-trip while the burst was applied is measured by the edits below.
    const e0 = performance.now()
    report('@@b5-edits')
    const rtts = []
    const tEdits = await until(async () => {
      const s = performance.now()
      const p = await invoke('list_pages', { filter: { categoryIds: [cat.id], limit: 1 } })
      rtts.push(performance.now() - s)
      const ps = (await all()).filter((x) => x.path?.startsWith('Projects/Burst/'))
      if (ps.length < 500) return false
      const bodies = await invoke('get_page', { id: ps[ps.length - 1].id })
      return bodies.body.includes('EDITED')
    }, 30000)
    await sleep(3000)
    const e1 = performance.now()
    const ps = passesBetween(e0, e1)
    const gaps = ps.slice(1).map((c, i) => Math.round(c.t - ps[i].t))
    report(`B5 edits over 5 s: last one in the index after ${tEdits} ms; knowledge passes: ${ps.length}; gaps between passes (ms): ${gaps.join(' ')}`)
    report(`B5 command round-trips while applying: n=${rtts.length}, median ${Math.round(rtts.sort((a, b) => a - b)[rtts.length >> 1])} ms, max ${Math.round(Math.max(...rtts))} ms`)
    const sample = (await all()).filter((x) => x.path?.startsWith('Projects/Burst/'))
    let edited = 0
    for (const p of sample.slice(0, 500)) if ((await invoke('get_page', { id: p.id })).body.includes('EDITED')) edited++
    report(`B5 check: ${sample.length} burst pages, ${edited} with the edit; groups: ${(await invoke('list_groups', { categoryId: cat.id })).map((g) => g.path).join(', ')}`)
    report('@@b5-done')
    await sleep(12000)

    // B6: renames made outside keep the id, a task link and the history.
    const task = (await invoke('list_tasks', { filter: {} }))[0]
    await invoke('link_task_page', { taskId: task.id, pageId: rust.id })
    const before6 = { versions: (await invoke('list_page_versions', { pageId: rust.id })).length, links: (await invoke('list_task_page_links', { taskId: null, pageId: rust.id })).length }
    report(`B6 before: ${rust.path} id=${rust.id}, versions=${before6.versions}, task links=${before6.links} (${task.key})`)
    report('@@b6-rename')
    const tRen = await until(async () => (await invoke('get_page', { id: rust.id }).catch(() => null))?.path === 'Learning/Rust ownership (renamed).md')
    const after = await invoke('get_page', { id: rust.id }).catch((e) => ({ error: JSON.stringify(e) }))
    report(`B6 rename in the folder: same id after ${tRen} ms: path=${after.path}, title=${after.title}, versions=${(await invoke('list_page_versions', { pageId: rust.id })).length}, task links=${(await invoke('list_task_page_links', { taskId: null, pageId: rust.id })).length}; pages titled like it: ${(await all()).filter((p) => p.title.startsWith('Rust ownership')).map((p) => p.id + ' ' + p.path).join(' | ')}`)
    report('@@b6-move')
    const tMove = await until(async () => (await invoke('get_page', { id: rust.id }).catch(() => null))?.path === 'Operations/Rust ownership (renamed).md')
    const moved = await invoke('get_page', { id: rust.id }).catch((e) => ({ error: JSON.stringify(e) }))
    const cats = await invoke('list_categories', { includeArchived: false })
    report(`B6 move to another folder: same id after ${tMove} ms: path=${moved.path}, category=${cats.find((c) => c.id === moved.categoryId)?.name}, task links=${(await invoke('list_task_page_links', { taskId: null, pageId: rust.id })).length}`)
    const zh = await byTitle('中文笔记')
    report('@@b6-zh')
    await sleep(4000)
    const zhAfter = await invoke('get_page', { id: zh.id }).catch(() => null)
    report(`B6 rename + edit at once (expected: a new page): old id still there=${!!zhAfter}; pages now: ${(await all()).filter((p) => p.path?.startsWith('Learning/')).map((p) => p.title + ' ' + p.id.slice(-6)).join(' | ')}`)
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
