// D-70 probe: changing many tasks at once in the List view (FR-TASK-UI-7). Run with the demo data;
// it opens /tasks itself:
//   CAIRN_DATA_DIR=/tmp/x PROBE_AFTER=8 PROBE_EXIT=80 scripts/ui-probe.sh scripts/probes/bulk-tasks.js
// Ticks rows (one, then a Shift+click range), sets them to Done through the bar, checks the 5-second
// countdown, adds a label, cancels a delete, then deletes two tasks. Reports what the data says.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const rowBoxes = () => [...document.querySelectorAll('li button[role="checkbox"][aria-label^="Select "]')]
const tickedKeys = () => rowBoxes().filter((b) => b.dataset.state === 'checked').map((b) => b.getAttribute('aria-label').slice(7))
const bar = () => document.querySelector('[role="toolbar"][aria-label="Selection"]')
const barButton = (t) => [...(bar()?.querySelectorAll('button') ?? [])].find((b) => text(b).startsWith(t))
const press = (el) => {
  el.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }))
  el.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, button: 0, pointerType: 'mouse' }))
}
const menuItem = (t) => [...document.querySelectorAll('[role="menuitem"]')].find((m) => text(m) === t)
const dialog = () => document.querySelector('[role="dialog"]')
const confirmBtn = () => dialog()?.querySelector('[data-testid="bulk-confirm"]')
const type = (el, v) => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v)
  el.dispatchEvent(new Event('input', { bubbles: true }))
}
const tasksByKey = async () => new Map((await invoke('list_tasks', { filter: { includeClosed: true } })).map((t) => [t.key, t]))

;(async () => {
  try {
    history.pushState({}, '', '/tasks')
    dispatchEvent(new PopStateEvent('popstate'))
    await sleep(1500)
    report('rows: ' + rowBoxes().length + ', header: ' + text(document.querySelector('label:has([aria-label="Select all shown tasks"])')))

    // One row, then Shift+click two rows further: three ticked.
    rowBoxes()[0].click()
    await sleep(200)
    rowBoxes()[2].dispatchEvent(new MouseEvent('click', { bubbles: true, shiftKey: true }))
    await sleep(300)
    const keys = tickedKeys()
    report('ticked after Shift+click: ' + keys.join(',') + ' | bar: ' + text(bar()?.querySelector('[data-testid="bulk-count"]')))
    report('url still /tasks: ' + location.pathname)

    // Status → Done, with the countdown.
    press(barButton('Status'))
    await sleep(400)
    menuItem('Done').click()
    await sleep(400)
    report('dialog: ' + text(dialog()?.querySelector('h2')) + ' | items: ' + dialog()?.querySelectorAll('[data-testid="bulk-items"] li').length)
    report('confirm at open: "' + text(confirmBtn()) + '" disabled=' + confirmBtn()?.disabled)
    confirmBtn()?.click() // must do nothing while locked
    await sleep(2000)
    report('after 2 s: "' + text(confirmBtn()) + '" disabled=' + confirmBtn()?.disabled + ' (a click while locked did nothing: dialog open=' + !!dialog() + ')')
    await sleep(3300)
    report('after 5.3 s: "' + text(confirmBtn()) + '" disabled=' + confirmBtn()?.disabled)
    confirmBtn().click()
    await sleep(1500)
    let t = await tasksByKey()
    report('statuses now: ' + keys.map((k) => `${k}=${t.get(k)?.status}`).join(', ') + ' | dialog open=' + !!dialog())
    report('bar after Done (closed tasks are hidden, so the selection empties): ' + (bar() ? text(bar()) : 'gone'))

    // Select all shown, then Add label.
    document.querySelector('[aria-label="Select all shown tasks"]').click()
    await sleep(300)
    const all = tickedKeys()
    report('select all shown: ' + all.length + ' ticked of ' + rowBoxes().length + ' | header: ' + text(document.querySelector('label:has([aria-label="Select all shown tasks"])')))
    barButton('Add label').click()
    await sleep(400)
    report('add label: confirm disabled with no label=' + confirmBtn()?.disabled)
    type(dialog().querySelector('input[aria-label="The label to add"]'), 'bulk-probe')
    await sleep(5400)
    report('add label after 5.4 s: "' + text(confirmBtn()) + '" disabled=' + confirmBtn()?.disabled)
    confirmBtn().click()
    await sleep(1500)
    t = await tasksByKey()
    report('with label bulk-probe: ' + [...t.values()].filter((x) => x.labels.includes('bulk-probe')).length + ' of ' + all.length)

    // Delete two, cancelled first.
    document.querySelector('[aria-label="Select all shown tasks"]').click() // clears
    await sleep(300)
    rowBoxes()[0].click()
    rowBoxes()[1].click()
    await sleep(300)
    const gone = tickedKeys()
    barButton('Delete').click()
    await sleep(400)
    report('delete dialog: ' + text(dialog()?.querySelector('h2')) + ' | "' + text(confirmBtn()) + '"')
    ;[...dialog().querySelectorAll('button')].find((b) => text(b) === 'Cancel').click()
    await sleep(500)
    t = await tasksByKey()
    report('after Cancel: ' + gone.map((k) => `${k} ${t.has(k) ? 'kept' : 'GONE'}`).join(', '))
    barButton('Delete').click()
    await sleep(5500)
    confirmBtn().click()
    await sleep(1500)
    t = await tasksByKey()
    report('after Delete: ' + gone.map((k) => `${k} ${t.has(k) ? 'KEPT' : 'deleted'}`).join(', ') + ' | bar: ' + (bar() ? 'shown' : 'gone') + ' | tasks left: ' + t.size)
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
