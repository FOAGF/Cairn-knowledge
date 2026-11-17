// The spelling tooltip of a comment box, shown near the window's bottom edge (so it opens above
// the word), then measured again: in WebKitGTK, CodeMirror turns its popups from `position: fixed`
// to `absolute` on the second measure, and an editor box that clipped them showed an empty frame.
// Reports where the tooltip lives and how many points of it are actually visible.
// Run on a fresh seeded data folder:
//   CAIRN_DATA_DIR=/tmp/x PROBE_AFTER=10 PROBE_EXIT=40 scripts/ui-probe.sh scripts/probes/popup-clip.js
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1500)
}

/** Points on a grid over `el` that `elementFromPoint` finds inside it. */
const visible = (el) => {
  const r = el.getBoundingClientRect()
  let seen = 0
  let all = 0
  for (let i = 1; i < 10; i++)
    for (let j = 1; j < 10; j++) {
      const x = r.left + (r.width * i) / 10
      const y = r.top + (r.height * j) / 10
      if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) continue
      all++
      if (el.contains(document.elementFromPoint(x, y))) seen++
    }
  return `${seen}/${all}`
}

const hover = async (view) => {
  const mark = view.dom.querySelector('.cm-lintRange-error')
  if (!mark) return null
  const r = mark.getBoundingClientRect()
  const at = { clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, bubbles: true }
  mark.dispatchEvent(new MouseEvent('mousemove', at))
  await sleep(150)
  mark.dispatchEvent(new MouseEvent('mousemove', at))
  await sleep(800)
  return document.querySelector('.cm-tooltip-lint')
}

const describe = (tip, word) => {
  const r = tip.getBoundingClientRect()
  const popup = tip.parentElement.closest('.cm-tooltip') ?? tip
  const where = tip.closest('.cm-editor') ? 'inside the editor' : tip.closest('[role="dialog"]') ? 'in the dialog' : 'in the body'
  return `${getComputedStyle(popup).position}, ${where}, ${r.top < word.top ? 'above' : 'below'} the word, ${Math.round(r.width)}x${Math.round(r.height)}, visible ${visible(tip)}`
}

;(async () => {
  try {
    const task = (await invoke('list_tasks', { filter: {} }))[0]
    await go(`/tasks/${task.key}`)
    const editors = document.querySelectorAll('.cm-content')
    const cm = editors[editors.length - 1]
    const view = cm.cmTile.root.view
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: 'This task got throw over the fanse' } })
    // The first check loads Harper's dictionary and takes a few seconds.
    for (let i = 0; i < 40 && !view.dom.querySelector('.cm-lintRange-error'); i++) await sleep(250)
    // The word near the window's bottom edge: the tooltip has no room below and opens above.
    cm.scrollIntoView({ block: 'end' })
    scrollBy(0, 40)
    await sleep(500)
    const word = () => view.dom.querySelector('.cm-lintRange-error')?.getBoundingClientRect()
    if (!word()) return report('no spelling mark (is the spelling check on?)')

    let tip = await hover(view)
    if (!tip) return report('no tooltip after hovering')
    report(`first measure: ${describe(tip, word())}`)
    // A second measure (any resize or scroll) is when CodeMirror switches to absolute.
    dispatchEvent(new Event('resize'))
    await sleep(400)
    tip = document.querySelector('.cm-tooltip-lint')
    report(tip ? `after a second measure: ${describe(tip, word())}` : 'tooltip gone after the second measure')
    // Leave and hover again: a new tooltip, now created absolute from the start.
    view.dom.dispatchEvent(new MouseEvent('mouseleave', { bubbles: false }))
    document.body.dispatchEvent(new MouseEvent('mousemove', { clientX: 5, clientY: 5, bubbles: true }))
    await sleep(800)
    tip = await hover(view)
    report(tip ? `hovered again: ${describe(tip, word())}` : 'no tooltip on the second hover')
    // A suggestion still replaces the word.
    tip.querySelector('.cm-diagnosticAction')?.click()
    await sleep(300)
    report(`after clicking the first suggestion: "${view.state.doc.toString()}"`)

    // The new-task dialog: the popups go into the dialog, so a click on them is inside it.
    await go(`/tasks?new=1`)
    const dialog = document.querySelector('[role="dialog"]')
    const dcm = dialog?.querySelector('.cm-content')
    if (!dcm) return report('no editor in the new-task dialog')
    const dview = dcm.cmTile.root.view
    dview.dispatch({ changes: { from: 0, insert: 'The fanse' } })
    for (let i = 0; i < 40 && !dview.dom.querySelector('.cm-lintRange-error'); i++) await sleep(250)
    const dword = dview.dom.querySelector('.cm-lintRange-error')?.getBoundingClientRect()
    tip = await hover(dview)
    report(tip ? `dialog: ${describe(tip, dword)}` : 'dialog: no tooltip')
    tip?.querySelector('.cm-diagnosticAction')?.click()
    await sleep(300)
    report(`dialog after the click: "${dview.state.doc.toString()}", dialog still open: ${document.contains(dialog)}`)
  } catch (e) {
    report(`error: ${e}`)
  }
  report('done')
})()
