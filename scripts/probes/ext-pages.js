// D-72 probe, external mode on a copy of the sample folder: the tree, an image, editing and saving,
// a tag edit while editing, the save conflict (both answers), "Saved as" with a rename, and delete.
// Run with its driver, which changes files outside Cairn and prints them when asked:
//   ROOT=/tmp/x-notes; cp -r crates/cairn-core/tests/fixtures/external-sample "$ROOT"
//   CAIRN_DATA_DIR=/tmp/x CAIRN_DEV_KNOWLEDGE_MODE=external=$ROOT CAIRN_DEV_TRASH_DIR=/tmp/x-trash \
//     ROOT=$ROOT TRASH=/tmp/x-trash SHOTS=/tmp/shots PROBE_AFTER=10 PROBE_EXIT=150 \
//     PROBE_DRIVER=scripts/probes/ext-pages-driver.sh scripts/probes/ext-probe.sh scripts/probes/ext-pages.js
// Steps (report prefixes): B1 tree and skipped file, B2 image, B3+B12 edit with a tag change while
// editing, B7 conflict (keep mine, then take the disk version), B10 "Saved as" and the rename,
// B11 delete. `@@word` lines are requests for the driver (ext-pages-driver.sh).
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1000)
}
const button = (t) => [...document.querySelectorAll('button')].find((b) => text(b) === t)
const editing = () => !!document.querySelector('[role="toolbar"][aria-label="Editing"]')
const view = () => document.querySelector('.cm-content')?.cmTile?.root?.view
const typeAtEnd = (s) => {
  const v = view()
  v.dispatch({ changes: { from: v.state.doc.length, insert: s }, selection: { anchor: v.state.doc.length + s.length } })
}
const setInput = (el, v) => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v)
  el.dispatchEvent(new Event('input', { bubbles: true }))
}
const all = async () => invoke('list_pages', { filter: { includeArchived: true } })
const byTitle = async (t) => (await all()).find((p) => p.title === t)
const conflictShown = () => !!$('save-conflict-dialog')
const article = () => text(document.querySelector('article'))

;(async () => {
  try {
    // B1: the tree.
    let pages = []
    for (let i = 0; i < 40 && pages.length < 7; i++) {
      pages = await all()
      await sleep(500)
    }
    const cats = await invoke('list_categories', { includeArchived: false })
    const catName = (id) => cats.find((c) => c.id === id)?.name
    const groups = await invoke('list_groups', { categoryId: null })
    const gName = (id) => groups.find((g) => g.id === id)?.name ?? '-'
    report('B1 categories: ' + cats.map((c) => c.name + (c.folder ? '→' + c.folder + '/' : '')).join(', '))
    report('B1 groups: ' + groups.map((g) => `${g.name} (path ${g.path}, in ${catName(g.categoryId)}${g.parentId ? ' › ' + gName(g.parentId) : ''})`).join('; '))
    report('B1 pages: ' + pages.map((p) => `${p.title} [${catName(p.categoryId)} › ${gName(p.groupId)}; ${p.path}; tags ${p.tags.join('+') || '-'}; source ${p.sourceInfo ?? '-'}${p.archived ? '; archived' : ''}]`).join(' | '))
    await go('/knowledge')
    await sleep(1000)
    report('B1 tree: ' + text(document.querySelector('aside')))
    report('B1 toasts: ' + [...document.querySelectorAll('[data-sonner-toast]')].map(text).join(' / '))
    report('@@shot-tree')
    await sleep(2500)
    await go('/settings/general')
    await sleep(800)
    report('B1 storage card: root=' + text($('knowledge-root')) + ' | skipped: ' + text($('knowledge-skipped')))

    // B2: the image of Pump check.
    const pump = await byTitle('Pump check')
    await go('/knowledge/' + pump.id)
    await sleep(1500)
    const imgs = [...document.querySelectorAll('article img')]
    report('B2 images: ' + imgs.map((i) => `${i.getAttribute('src')} complete=${i.complete} natural=${i.naturalWidth}x${i.naturalHeight}`).join(' | '))
    report('B2 index body: ' + JSON.stringify((await invoke('get_page', { id: pump.id })).body))
    report('B2 page path line: ' + text($('page-path')))
    report('@@shot-image')
    await sleep(2500)
    const welcome = await byTitle('Welcome')
    await go('/knowledge/' + welcome.id)
    report('B2 Welcome links: ' + [...document.querySelectorAll('article a')].map((a) => `${text(a)}→${a.getAttribute('href')}`).join(' | ') + ' | chips: ' + [...document.querySelectorAll('article [data-page-chip], article button')].map(text).filter(Boolean).slice(0, 6).join(' | '))
    report('B2 Welcome index body: ' + JSON.stringify((await invoke('get_page', { id: welcome.id })).body))

    // B3 + B12: edit Overview, change its tags in the sidebar while editing, then Save.
    const overview = await byTitle('Overview')
    const seal = await byTitle('Seal replacement')
    await go('/knowledge/' + overview.id)
    button('Edit').click()
    await sleep(700)
    typeAtEnd(`\n\nEdited in Cairn: see [the seals](page:${seal.id}) and ![Flow](folder:Operations/diagrams/flow.png).\n`)
    await sleep(400)
    const removeSite = document.querySelector('[aria-label="Remove tag site"]')
    report('B12 editing=' + editing() + ', remove-tag button=' + !!removeSite)
    removeSite?.click()
    await sleep(1500)
    const tagInput = [...document.querySelectorAll('input')].find((i) => /^Add/.test(i.placeholder))
    if (tagInput) {
      setInput(tagInput, 'edited')
      tagInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    }
    await sleep(1500)
    const midway = await invoke('get_page', { id: overview.id })
    report('B12 after tag edits (still editing=' + editing() + '): index tags=' + midway.tags.join('+'))
    button('Save').click()
    await sleep(2000)
    report('B12 after Save: conflict dialog=' + conflictShown() + ', editing=' + editing())
    const saved = await invoke('get_page', { id: overview.id })
    report('B3 saved: tags=' + saved.tags.join('+') + ' source=' + saved.sourceInfo + ' body has edit=' + saved.body.includes('Edited in Cairn'))
    await invoke('update_page', { id: (await byTitle('Rust ownership')).id, patch: { sourceInfo: 'The Rust book, ch. 4' } })
    await sleep(800)
    report('@@dump-b3')
    await sleep(1500)

    // B7: save conflict on Seal replacement: Keep mine.
    await go('/knowledge/' + seal.id)
    button('Edit').click()
    await sleep(700)
    typeAtEnd('\nMINE-1 typed in Cairn.\n')
    await sleep(300)
    report('@@outside-seal-1')
    await sleep(3000)
    report('B7a before Save: editing=' + editing() + ', editor has MINE-1=' + (view()?.state.doc.toString().includes('MINE-1') ?? false) + ', index has OUTSIDE-1=' + (await invoke('get_page', { id: seal.id })).body.includes('OUTSIDE-1'))
    button('Save').click()
    await sleep(1500)
    report('B7a after Save: conflict dialog=' + conflictShown() + ' "' + text($('save-conflict-dialog')).slice(0, 160) + '"')
    $('conflict-show-diff')?.click()
    await sleep(1500)
    report('B7a diff: ' + text($('conflict-diff')).slice(0, 300))
    report('@@shot-conflict')
    await sleep(2500)
    $('conflict-keep-mine')?.click()
    await sleep(2000)
    report('B7a after Keep mine: dialog=' + conflictShown() + ', editing=' + editing())
    report('@@dump-seal-1')
    await sleep(1500)

    // B7: again, and Take the disk version.
    button('Edit').click()
    await sleep(700)
    typeAtEnd('\nMINE-2 typed in Cairn.\n')
    await sleep(300)
    report('@@outside-seal-2')
    await sleep(3000)
    button('Save').click()
    await sleep(1500)
    report('B7b after Save: conflict dialog=' + conflictShown())
    $('conflict-take-disk')?.click()
    await sleep(2000)
    const sealNow = await invoke('get_page', { id: seal.id })
    report('B7b after Take disk: dialog=' + conflictShown() + ', editing=' + editing() + ', shown has OUTSIDE-2=' + article().includes('OUTSIDE-2') + ', shown has MINE-2=' + article().includes('MINE-2') + ', index has OUTSIDE-2=' + sealNow.body.includes('OUTSIDE-2'))
    report('@@dump-seal-2')
    await sleep(1500)

    // B10: "Saved as" for a title with ":" and a leading dot, then Save renames the file.
    await go('/knowledge/' + pump.id)
    button('Edit').click()
    await sleep(700)
    const titleBox = document.querySelector('input[aria-label="Title"]')
    setInput(titleBox, '.Pump: check?')
    await sleep(500)
    report('B10 title box "' + titleBox.value + '" → ' + text($('saved-as')))
    report('@@shot-savedas')
    await sleep(2500)
    button('Save').click()
    await sleep(2500)
    const renamed = await invoke('get_page', { id: pump.id })
    report('B10 after Save: conflict=' + conflictShown() + ', editing=' + editing() + ', title="' + renamed.title + '", path=' + renamed.path + ', h1="' + text(document.querySelector('article h1')) + '"')
    report('@@dump-rename')
    await sleep(1500)

    // B11: delete Hose guide.
    const hose = await byTitle('Hose guide')
    await go('/knowledge/' + hose.id)
    document.querySelector('button[aria-label="Delete page"]').click()
    await sleep(600)
    const dlg = document.querySelector('[role="alertdialog"], [role="dialog"]')
    report('B11 dialog: ' + text(dlg).slice(0, 200))
    ;[...dlg.querySelectorAll('button')].find((b) => text(b) === 'Delete page').click()
    await sleep(2500)
    report('B11 after delete: in index=' + !!(await byTitle('Hose guide')) + ', url=' + location.pathname)
    report('@@dump-trash')
    await sleep(1500)
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
