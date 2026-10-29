// Holding Backspace in the app's text boxes: one Backspace per frame for 3 s in each, reporting the
// frame time. A held key repeats every 30 ms on the owner's machine; a box whose frames take longer
// falls behind and keeps deleting after the key is released.
// Run on a copy of a seeded data folder (it edits but never saves):
//   CAIRN_DATA_DIR=/tmp/x PROBE_AFTER=8 PROBE_EXIT=120 scripts/ui-probe.sh scripts/probes/key-repeat-surfaces.js
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1500)
}
const button = (label, root = document) => [...root.querySelectorAll('button')].find((b) => b.textContent.trim() === label)
const text = 'Ownership in Rust means each value has one owner, and the value is dropped when the owner goes out of scope. '.repeat(30)

/** Deletes one character per frame for `ms` with `press`; returns the mean and worst frame time. */
const burst = async (press, ms = 3000) => {
  const stamps = []
  const end = performance.now() + ms
  await new Promise((done) => {
    const step = (t) => {
      stamps.push(t)
      if (performance.now() > end) return done()
      press()
      requestAnimationFrame(step)
    }
    requestAnimationFrame(step)
  })
  const gaps = stamps.slice(1).map((t, i) => t - stamps[i])
  const mean = gaps.reduce((a, b) => a + b, 0) / gaps.length
  return `${mean.toFixed(1)} ms per frame (worst ${Math.max(...gaps).toFixed(0)} ms)`
}
/** Backspace in a CodeMirror editor. */
const inEditor = (cm) => {
  const v = cm.cmTile.root.view
  v.dispatch({ changes: { from: v.state.doc.length, insert: text }, selection: { anchor: v.state.doc.length + text.length } })
  return () => v.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', code: 'Backspace', keyCode: 8, bubbles: true, cancelable: true }))
}
/** Backspace in a React-controlled input or textarea. */
const inField = (el) => {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype
  const set = Object.getOwnPropertyDescriptor(proto, 'value').set
  set.call(el, el.value + text.slice(0, 400))
  el.dispatchEvent(new InputEvent('input', { bubbles: true }))
  return () => {
    set.call(el, el.value.slice(0, -1))
    el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }))
  }
}

;(async () => {
  try {
    const task = (await invoke('list_tasks', { filter: {} }))[0]

    await go('/assistant')
    const composer = document.querySelector('textarea')
    report(`Assistant message box: ${await burst(inField(composer))}`)

    await go(`/tasks/${task.key}`)
    const editors = document.querySelectorAll('.cm-content')
    report(`task: new comment editor: ${await burst(inEditor(editors[editors.length - 1]))}`)

    await go('/tasks')
    button('New task').click()
    await sleep(1000)
    const dialog = document.querySelector('[role="dialog"]')
    report(`new task dialog, description: ${await burst(inEditor(dialog.querySelector('.cm-content')))}`)
    report(`new task dialog, title: ${await burst(inField(dialog.querySelector('input')))}`)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await sleep(800)

    await go('/knowledge')
    report(`knowledge tree search: ${await burst(inField(document.querySelector('input[placeholder^="Search title"]')))}`)
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
