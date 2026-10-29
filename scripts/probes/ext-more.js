// D-72 probe, follow-ups on the data folder ext-pages.js left (run it again on that folder, with its
// driver ext-more-driver.sh, which changes the folder while Cairn is closed first):
//   CAIRN_DATA_DIR=/tmp/x ROOT=$ROOT PROBE_AFTER=10 PROBE_EXIT=60 \
//     PROBE_DRIVER=scripts/probes/ext-more-driver.sh scripts/probes/ext-probe.sh scripts/probes/ext-more.js
// M1 the renamed page's `# title` line after ext-pages.js's B10 (shown twice?), M2 changes made while
// Cairn was closed (the start-up rescan), M3 a new page in General and in a group, M4 a title that
// matches another file's name in other capitals, M5 archive (front matter), M6 a pasted image in
// `attachments`, and removing the last use of it and of an image outside `attachments`.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1200)
}
const all = async () => invoke('list_pages', { filter: { includeArchived: true } })
const byTitle = async (t) => (await all()).find((p) => p.title === t)
const err = (e) => e?.message ?? JSON.stringify(e)

;(async () => {
  try {
    for (let i = 0; i < 30 && (await all()).length < 5; i++) await sleep(500)
    await sleep(4000) // the start-up scan begins 3 s after the window opens
    // M1
    const pump = await byTitle('Pump- check-')
    await go('/knowledge/' + pump.id)
    report('M1 headings on the page: ' + [...document.querySelectorAll('article h1, article h2')].map((h) => h.tagName + ' "' + text(h) + '"').join(' | '))
    report('@@shot-title')
    await sleep(2000)
    // M2
    const pages = await all()
    report('M2 after the start-up scan: ' + pages.map((p) => p.path).sort().join(', '))
    report('M2 Welcome body has CLOSED-EDIT=' + (await invoke('get_page', { id: (await byTitle('Welcome')).id })).body.includes('CLOSED-EDIT') + ', Old notes still there=' + !!(await byTitle('Old notes')))
    // M3
    const cats = await invoke('list_categories', { includeArchived: false })
    const general = cats.find((c) => c.name === 'General')
    const ops = cats.find((c) => c.name === 'Operations')
    const pumps = (await invoke('list_groups', { categoryId: ops.id })).find((g) => g.name === 'Pumps')
    const a = await invoke('create_page', { input: { categoryId: general.id, title: 'New in General', body: 'Body.', tags: ['fresh'] } })
    const b = await invoke('create_page', { input: { categoryId: ops.id, groupId: pumps.id, title: 'CON', body: 'A reserved name.' } })
    report(`M3 new pages: "${a.title}" → ${a.path}; "${b.title}" → ${b.path}`)
    // M4
    const learning = cats.find((c) => c.name === 'Learning')
    const m4 = await invoke('create_page', { input: { categoryId: learning.id, title: 'rust OWNERSHIP', body: 'x' } }).then((p) => 'created ' + p.path, (e) => 'refused: ' + err(e))
    report('M4 "rust OWNERSHIP" next to "Rust ownership.md": ' + m4)
    // M5
    const zh = await byTitle('中文笔记')
    await invoke('archive_page', { id: zh.id, archived: true })
    report('@@dump-archive')
    await sleep(1500)
    // M6: a pasted image, used by a page, then not used; and the last use of diagrams/flow.png.
    const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='
    const img = await invoke('save_page_image', { categoryId: ops.id, dataBase64: png, mime: 'image/png' })
    report('M6 saved image: ' + JSON.stringify(img))
    const overview = await byTitle('Overview')
    const ov = await invoke('get_page', { id: overview.id })
    const withImg = await invoke('update_page', { id: overview.id, patch: { body: ov.body + `\n![pasted](${img.target})\n`, baseHash: ov.fileHash } })
    report('M6 page with the image: ' + JSON.stringify(withImg.body.slice(-120)))
    report('@@dump-images-1')
    await sleep(1500)
    const now = await invoke('get_page', { id: overview.id })
    const without = now.body.replace(/\n!\[pasted\]\([^)]*\)\n/, '\n').replace(/ and !\[Flow\]\([^)]*\)/, '')
    await invoke('update_page', { id: overview.id, patch: { body: without, baseHash: now.fileHash } })
    report('M6 page without either image: ' + JSON.stringify((await invoke('get_page', { id: overview.id })).body.slice(-160)))
    await sleep(2000)
    report('@@dump-images-2')
    await sleep(1500)
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
