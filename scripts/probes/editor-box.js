// The editor's contained box (MarkdownEditor.tsx `followHeight`) and what changed with it: the box
// keeps the editor's height (lines added and removed, Preview and back), the popups still open next to
// the cursor, glossary terms are still marked (once typing pauses), and the pending tab's title follows
// a changed title. Run on a fresh seeded data folder (it edits, never saves):
//   CAIRN_DATA_DIR=/tmp/x PROBE_AFTER=8 PROBE_EXIT=60 scripts/ui-probe.sh scripts/probes/editor-box.js
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1200)
}
const view = () => document.querySelector('.cm-content')?.cmTile?.root?.view
const button = (label) => [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === label)
const heights = () => {
  const v = view()
  const box = v.dom.parentElement
  return `box ${box.getBoundingClientRect().height.toFixed(1)} / editor ${v.dom.getBoundingClientRect().height.toFixed(1)} (contain: ${getComputedStyle(box).contain})`
}
const type = (v, text) => v.dispatch({ changes: { from: v.state.selection.main.head, insert: text }, selection: { anchor: v.state.selection.main.head + text.length }, userEvent: 'input.type' })
const tabText = () => [...document.querySelectorAll('header a')].find((a) => a.querySelector('[aria-label="Not saved"]'))?.textContent.trim() ?? '(no tab)'

;(async () => {
  try {
    const page = (await invoke('list_pages', { filter: {} }))[0]
    await go(`/knowledge/${page.id}`)
    button('Edit').click()
    await sleep(1000)
    let v = view()
    report(`start: ${heights()}`)
    const before = v.state.doc.length
    v.dispatch({ selection: { anchor: before } })
    type(v, '\n' + Array.from({ length: 40 }, (_, i) => `Line ${i}`).join('\n'))
    await sleep(400)
    report(`40 lines added: ${heights()}`)
    v.dispatch({ changes: { from: before, to: v.state.doc.length } })
    await sleep(400)
    report(`removed again: ${heights()}`)
    const toggle = (m) => [...document.querySelectorAll('[aria-label="Editor mode"] button')].find((b) => b.textContent.trim() === m).click()
    toggle('preview')
    await sleep(500)
    toggle('edit')
    await sleep(500)
    report(`after Preview and back: ${heights()}`)

    // The page-link popup at the end of the text.
    v.focus()
    v.dispatch({ selection: { anchor: v.state.doc.length } })
    type(v, '\n[[')
    await sleep(800)
    const popup = document.querySelector('.cm-tooltip-autocomplete')
    if (!popup) report('popup: NOT SHOWN')
    else {
      const p = popup.getBoundingClientRect()
      const c = v.coordsAtPos(v.state.doc.length)
      report(`popup: ${popup.querySelectorAll('li').length} items, top ${p.top.toFixed(0)} (cursor bottom ${c.bottom.toFixed(0)}), left ${p.left.toFixed(0)} (cursor ${c.left.toFixed(0)}), ${p.height.toFixed(0)} px tall, inside the window: ${p.bottom <= innerHeight && p.top >= 0}, in the editor's DOM: ${v.dom.contains(popup)}`)
    }
    v.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))

    // A glossary term: marked once typing pauses.
    const term = (await invoke('list_glossary'))[0]?.term
    if (term) {
      const marked = () => [...v.contentDOM.querySelectorAll('.cm-glossary-term')].map((e) => e.textContent)
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: `Something about ${term} here.` } })
      await sleep(50)
      const soon = marked()
      await sleep(600)
      report(`glossary "${term}": marked right after typing ${JSON.stringify(soon)}, after the pause ${JSON.stringify(marked())}`)
    }

    // The pending tab's title follows the title field once typing pauses.
    const title = document.querySelector('input[aria-label="Title"]')
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set
    set.call(title, 'Renamed while editing')
    title.dispatchEvent(new InputEvent('input', { bubbles: true }))
    await sleep(100)
    const early = tabText()
    await sleep(600)
    report(`tab after a title change: at 0.1 s "${early}", at 0.7 s "${tabText()}"`)
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
