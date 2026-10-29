// D-72 probe, external mode on a copy of the sample folder: the bulk *Move* with file-name clashes
// (FR-KN-EXT-10) and nested groups (*New group inside*, *Move to…*, *Delete group*). Run with its
// driver, which prints the folder when asked:
//   CAIRN_DATA_DIR=/tmp/x CAIRN_DEV_KNOWLEDGE_MODE=external=$ROOT ROOT=$ROOT SHOTS=/tmp/shots \
//     PROBE_AFTER=10 PROBE_EXIT=120 PROBE_DRIVER=scripts/probes/ext-groups-driver.sh \
//     scripts/probes/ext-probe.sh scripts/probes/ext-groups.js
// B8: Overview, Notes (Pumps) and Notes (Hoses) moved into Learning, which has an Overview already:
//     two clashes listed, *Move* stays locked past its countdown, `update_pages` refuses the whole
//     change; then a clash-free move of one page works and moves its file.
// B9: *New group inside* Hoses, *Move to…* Seals into Hoses, *Delete group* Hoses (its contents move
//     up and its folder goes), *Delete group* Pumps with an image left in its folder (the folder stays).
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
const press = (el) => {
  el.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }))
  el.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, button: 0, pointerType: 'mouse' }))
}
const pick = async (trigger, optionText) => {
  press(trigger)
  await sleep(400)
  const opt = [...document.querySelectorAll('[role="option"]')].find((o) => text(o) === optionText)
  if (!opt) throw new Error('no option "' + optionText + '" in ' + [...document.querySelectorAll('[role="option"]')].map(text).join(' / '))
  press(opt)
  opt.click()
  await sleep(400)
}
const menu = async (label, item) => {
  press(document.querySelector(`[aria-label="${label}"]`))
  await sleep(400)
  const it = [...document.querySelectorAll('[role="menuitem"]')].find((m) => text(m) === item)
  if (!it) throw new Error('no menu item "' + item + '" in ' + [...document.querySelectorAll('[role="menuitem"]')].map(text).join(' / '))
  it.click()
  await sleep(600)
}
const setInput = (el, v) => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v)
  el.dispatchEvent(new Event('input', { bubbles: true }))
}
const dialog = () => document.querySelector('[role="dialog"]')
const all = async () => invoke('list_pages', { filter: { includeArchived: true } })
const groupsOf = async (catId) => invoke('list_groups', { categoryId: catId })
const tick = (id) => document.querySelector(`[data-page-row="${id}"]`).click()

;(async () => {
  try {
    let pages = []
    for (let i = 0; i < 40 && pages.length < 7; i++) {
      pages = await all()
      await sleep(500)
    }
    const cats = await invoke('list_categories', { includeArchived: false })
    const ops = cats.find((c) => c.name === 'Operations')
    const learning = cats.find((c) => c.name === 'Learning')
    let gs = await groupsOf(ops.id)
    const g = (name) => gs.find((x) => x.name === name)
    const p = (title) => pages.find((x) => x.title === title)
    await invoke('create_page', { input: { categoryId: learning.id, title: 'Overview', body: 'Learning has its own overview.' } })
    const notesP = await invoke('create_page', { input: { categoryId: ops.id, groupId: g('Pumps').id, title: 'Notes', body: 'Pump notes.' } })
    const notesH = await invoke('create_page', { input: { categoryId: ops.id, groupId: g('Hoses').id, title: 'Notes', body: 'Hose notes.' } })
    report('setup: Notes (Pumps) ' + notesP.path + ', Notes (Hoses) ' + notesH.path)
    await sleep(800)

    // B8: a move with clashes.
    await go('/knowledge')
    button('Select').click()
    await sleep(500)
    for (const id of [p('Overview').id, notesP.id, notesH.id]) tick(id)
    await sleep(400)
    report('B8 selected: ' + text($('bulk-count')))
    ;[...document.querySelectorAll('[role="toolbar"][aria-label="Selection"] button')].find((b) => text(b).startsWith('Move')).click()
    await sleep(700)
    await pick(dialog().querySelector('[aria-label="Target category"]'), 'Learning')
    await sleep(1500)
    report('B8 clashes: ' + text($('bulk-clashes')) + ' | rows=' + document.querySelectorAll('[data-testid="bulk-clash"]').length)
    await sleep(5000)
    report('B8 confirm after 6.5 s: "' + text($('bulk-confirm')) + '" disabled=' + $('bulk-confirm')?.disabled)
    report('@@shot-clash')
    await sleep(2500)
    const refused = await invoke('update_pages', { ids: [p('Overview').id, notesP.id, notesH.id], change: { kind: 'move', categoryId: learning.id, groupId: null } }).then(
      (r) => 'accepted ' + JSON.stringify(r),
      (e) => 'refused: ' + (e.message ?? JSON.stringify(e)),
    )
    const after = await all()
    report('B8 update_pages directly: ' + refused + ' | paths now: ' + [p('Overview').id, notesP.id, notesH.id].map((id) => after.find((x) => x.id === id)?.path).join(', '))
    button('Cancel')?.click()
    await sleep(500)
    // Clash-free: only Notes (Pumps).
    tick(p('Overview').id)
    tick(notesH.id)
    await sleep(400)
    report('B8 now selected: ' + text($('bulk-count')))
    ;[...document.querySelectorAll('[role="toolbar"][aria-label="Selection"] button')].find((b) => text(b).startsWith('Move')).click()
    await sleep(700)
    await pick(dialog().querySelector('[aria-label="Target category"]'), 'Learning')
    await sleep(1500)
    report('B8 clash-free: clashes shown=' + !!$('bulk-clashes'))
    await sleep(5000)
    report('B8 confirm "' + text($('bulk-confirm')) + '" disabled=' + $('bulk-confirm')?.disabled)
    $('bulk-confirm').click()
    await sleep(2000)
    const moved = await invoke('get_page', { id: notesP.id })
    report('B8 moved: Notes (Pumps) now ' + moved.path + ' in ' + cats.find((c) => c.id === moved.categoryId)?.name)
    report('@@dump-b8')
    await sleep(1500)
    button('Stop selecting')?.click()
    await sleep(500)

    // B9: New group inside Hoses.
    await menu('Actions for Hoses', 'New group inside')
    report('B9 dialog: ' + text(dialog()?.querySelector('h2')))
    setInput(dialog().querySelector('input'), 'Fittings')
    await sleep(200)
    button('Save').click()
    await sleep(1500)
    gs = await groupsOf(ops.id)
    report('B9 new group: ' + JSON.stringify(gs.filter((x) => x.name === 'Fittings').map((x) => ({ path: x.path, parent: gs.find((y) => y.id === x.parentId)?.name }))))
    report('@@dump-fittings')
    await sleep(1000)
    const fitting = await invoke('create_page', { input: { categoryId: ops.id, groupId: g('Fittings').id, title: 'Fitting sizes', body: 'Sizes. See [the seals](page:' + p('Seal replacement').id + ').' } })
    report('B9 page in Fittings: ' + fitting.path)

    // Move to…: Seals (inside Pumps) into Hoses.
    await menu('Actions for Seals', 'Move to…')
    report('B9 move dialog: ' + !!$('group-move-dialog') + ' ' + text($('group-move-dialog')?.querySelector('h2')))
    await pick($('group-move-dialog').querySelector('[aria-label="Move into"]'), 'Hoses')
    $('group-move-confirm').click()
    await sleep(2000)
    gs = await groupsOf(ops.id)
    pages = await all()
    report('B9 after Move to: Seals path=' + g('Seals').path + ' parent=' + gs.find((y) => y.id === g('Seals').parentId)?.name + ', Seal replacement path=' + p('Seal replacement').path)
    report('@@dump-move')
    await sleep(1500)

    // Delete Hoses: Hose guide, Notes (Hoses), Fittings and Seals move up into Operations.
    await menu('Actions for Hoses', 'Delete group')
    report('B9 delete dialog: ' + text(dialog()).slice(0, 220))
    ;[...dialog().querySelectorAll('button')].find((b) => text(b) === 'Delete group').click()
    await sleep(2500)
    gs = await groupsOf(ops.id)
    pages = await all()
    report('B9 after deleting Hoses: groups ' + gs.map((x) => `${x.name}(${x.path}${x.parentId ? ' in ' + gs.find((y) => y.id === x.parentId)?.name : ''})`).join(', ') + ' | pages ' + pages.filter((x) => x.categoryId === ops.id).map((x) => x.path).join(', '))
    report('@@dump-delete-hoses')
    await sleep(1500)

    // Delete Pumps with an image left in its folder.
    report('@@put-image')
    await sleep(2500)
    await menu('Actions for Pumps', 'Delete group')
    ;[...dialog().querySelectorAll('button')].find((b) => text(b) === 'Delete group').click()
    await sleep(2500)
    gs = await groupsOf(ops.id)
    pages = await all()
    report('B9 after deleting Pumps: groups ' + gs.map((x) => x.name + '(' + x.path + ')').join(', ') + ' | Pump check path=' + p('Pump check').path + ' | toasts: ' + [...document.querySelectorAll('[data-sonner-toast]')].map(text).join(' / '))
    report('@@dump-delete-pumps')
    await sleep(1500)
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
