// D-70 probe: changing many pages at once through *Select* on the knowledge tree (FR-KN-UI-3). Run
// with the demo data; it opens /knowledge itself:
//   CAIRN_DATA_DIR=/tmp/x PROBE_AFTER=8 PROBE_EXIT=90 scripts/ui-probe.sh scripts/probes/bulk-pages.js
// Ticks the Learning category (then unticks its Rust group), adds a tag, moves the pages into
// General › Admin with one title clash (skipped and named), archives and restores them, then
// deletes two. Each confirmation waits out its 5-second countdown. Reports what the data says.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const button = (t) => [...document.querySelectorAll('button')].find((b) => text(b) === t)
const box = (label) => document.querySelector(`button[role="checkbox"][aria-label="${label}"]`)
const pageRows = () => [...document.querySelectorAll('aside [data-page-row]')]
const bar = () => document.querySelector('[role="toolbar"][aria-label="Selection"]')
const barButton = (t) => [...(bar()?.querySelectorAll('button') ?? [])].find((b) => text(b).startsWith(t))
const barCount = () => text(bar()?.querySelector('[data-testid="bulk-count"]'))
const dialog = () => document.querySelector('[role="dialog"]')
const confirmBtn = () => dialog()?.querySelector('[data-testid="bulk-confirm"]')
const press = (el) => {
  el.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }))
  el.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, button: 0, pointerType: 'mouse' }))
}
const pick = async (trigger, optionText) => {
  press(trigger)
  await sleep(400)
  const opt = [...document.querySelectorAll('[role="option"]')].find((o) => text(o) === optionText)
  press(opt)
  opt.click()
  await sleep(300)
}
const type = (el, v) => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v)
  el.dispatchEvent(new Event('input', { bubbles: true }))
}
/** Waits out the countdown, then confirms. */
const confirmAfterCountdown = async () => {
  const before = text(confirmBtn())
  await sleep(5300)
  const ready = !confirmBtn().disabled
  confirmBtn().click()
  await sleep(1500)
  return `"${before}" → ready after 5.3 s: ${ready}`
}
const pages = async () => invoke('list_pages', { filter: { includeArchived: true } })

;(async () => {
  try {
    history.pushState({}, '', '/knowledge')
    dispatchEvent(new PopStateEvent('popstate'))
    await sleep(1500)
    const cats = await invoke('list_categories', { includeArchived: false })
    const general = cats.find((c) => c.name === 'General')
    const learning = cats.find((c) => c.name === 'Learning')
    const admin = (await invoke('list_groups', { categoryId: general.id })).find((g) => g.name === 'Admin')
    await invoke('create_page', { input: { categoryId: general.id, title: 'Tauri commands', body: 'Already in General, so the move skips the other one.' } })
    await sleep(800)

    report('before Select: checkboxes on pages=' + pageRows().length)
    button('Select').click()
    await sleep(400)
    report('after Select: page checkboxes=' + pageRows().length + ', button now "' + text(button('Stop selecting')) + '"')

    box('Select the pages of Learning').click()
    await sleep(300)
    report('Learning ticked: bar ' + barCount())
    box('Select the pages of Rust').click()
    await sleep(300)
    report('Rust unticked: bar ' + barCount() + ', Learning box ' + box('Select the pages of Learning').dataset.state)
    box('Select the pages of Learning').click()
    await sleep(300)
    const learningIds = (await pages()).filter((p) => p.categoryId === learning.id).map((p) => p.id)
    report('Learning ticked again: bar ' + barCount() + ', Learning pages ' + learningIds.length)

    // Add a tag.
    barButton('Add tag').click()
    await sleep(400)
    type(dialog().querySelector('input[aria-label="The tag to add"]'), 'bulk-tag')
    report('add tag: ' + (await confirmAfterCountdown()))
    let all = await pages()
    report('with bulk-tag: ' + all.filter((p) => p.tags.includes('bulk-tag')).length)

    // Move into General › Admin: "Tauri commands" clashes.
    barButton('Move').click()
    await sleep(400)
    await pick(dialog().querySelector('[aria-label="Target category"]'), 'General')
    await pick(dialog().querySelector('[aria-label="Target group"]'), 'Admin')
    report('move dialog: ' + text(dialog()?.querySelector('h2')) + ' | ' + (await confirmAfterCountdown()))
    report('result: ' + text(dialog()?.querySelector('h2')) + ' | ' + text(dialog()?.querySelector('[role="dialog"] p')) + ' | skipped: ' + text(dialog()?.querySelector('[data-testid="bulk-skipped"]')))
    button('Close')?.click()
    await sleep(500)
    all = await pages()
    const moved = learningIds.map((id) => all.find((p) => p.id === id))
    report('after move: ' + moved.map((p) => `${p.title}: ${p.categoryId === general.id ? 'General' : 'Learning'}${p.groupId === admin.id ? ' › Admin' : ''}`).join('; '))
    report('bar still: ' + barCount())

    // Archive: the pages leave the tree (Show archived is off), so the selection empties.
    barButton('Archive').click()
    await sleep(400)
    report('archive: ' + (await confirmAfterCountdown()))
    all = await pages()
    report('archived: ' + learningIds.filter((id) => all.find((p) => p.id === id).archived).length + ' of 4 | bar ' + (bar() ? barCount() : 'gone'))

    // Show archived, tick them again, Restore.
    ;[...document.querySelectorAll('label')].find((l) => text(l) === 'Show archived').querySelector('button').click()
    await sleep(800)
    report('after Show archived, still ticked: ' + (bar() ? barCount() : 'none (archiving took them out of the selection)'))
    for (const b of pageRows()) if (learningIds.includes(b.dataset.pageRow)) b.click()
    await sleep(300)
    report('ticked archived pages: bar ' + barCount() + ' | Restore shown: ' + !!barButton('Restore') + ' | Archive shown: ' + !!barButton('Archive'))
    barButton('Restore').click()
    await sleep(400)
    report('restore: ' + (await confirmAfterCountdown()))
    all = await pages()
    report('archived now: ' + learningIds.filter((id) => all.find((p) => p.id === id).archived).length)

    // Delete two.
    button('Stop selecting').click()
    await sleep(300)
    report('stopped: page checkboxes=' + pageRows().length + ', bar ' + (bar() ? 'shown' : 'gone'))
    button('Select').click()
    await sleep(300)
    const two = learningIds.slice(0, 2).map((id) => all.find((p) => p.id === id).title)
    for (const b of pageRows()) if (learningIds.slice(0, 2).includes(b.dataset.pageRow)) b.click()
    await sleep(300)
    barButton('Delete').click()
    await sleep(400)
    report('delete dialog: ' + text(dialog()?.querySelector('h2')) + ' | items: ' + text(dialog()?.querySelector('[data-testid="bulk-items"]')))
    report('delete: ' + (await confirmAfterCountdown()))
    all = await pages()
    report('after delete: ' + two.map((t) => `${t} ${all.some((p) => p.title === t && learningIds.includes(p.id)) ? 'KEPT' : 'deleted'}`).join(', ') + ' | pages left ' + all.length + ' | bar ' + (bar() ? 'shown' : 'gone'))
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
