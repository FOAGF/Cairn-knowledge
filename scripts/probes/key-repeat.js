// Holding Backspace: does each press, with the frame it causes, finish before the next repeat? On the
// owner's machine a held key repeats every 30 ms; frames that take longer fall behind, and the queued
// presses keep deleting after the key is released (see `followHeight` in MarkdownEditor.tsx).
// One Backspace per frame for a while, in the page editor, the page's title field and the task
// description editor: the frame time must stay well under 30 ms. The page editor also gets a
// per-press breakdown (key handler, React, style and layout, the wait for the next painted frame).
// Run on a fresh seeded data folder (it edits but never saves):
//   CAIRN_DATA_DIR=/tmp/x PROBE_AFTER=10 PROBE_EXIT=90 scripts/ui-probe.sh scripts/probes/key-repeat.js
// Optional, prepended: `const BODY_KB = 20` (the text's size; default 20). Compare with
// WEBKIT_DISABLE_DMABUF_RENDERER=0 (WebKit's own renderer, which Cairn turns off on Linux).
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1200)
}
const view = () => document.querySelector('.cm-content')?.cmTile?.root?.view
const frame = () => new Promise((r) => requestAnimationFrame(() => r()))
const backspace = (el) =>
  el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', code: 'Backspace', keyCode: 8, which: 8, bubbles: true, cancelable: true }))
const stats = (xs) => {
  const s = [...xs].sort((a, b) => a - b)
  const at = (q) => s[Math.min(s.length - 1, Math.floor(q * s.length))].toFixed(1)
  return `median ${at(0.5)} ms, p90 ${at(0.9)} ms, max ${s[s.length - 1].toFixed(1)} ms`
}
const med = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)].toFixed(1)

/** `press` once in every frame for `n` frames; the frame interval shows whether work and paint fit. */
const perFrame = async (press, n = 90) => {
  const stamps = []
  await new Promise((done) => {
    const step = (t) => {
      stamps.push(t)
      if (stamps.length > n) return done()
      press()
      requestAnimationFrame(step)
    }
    requestAnimationFrame(step)
  })
  return `frame time with a Backspace in each: ${stats(stamps.slice(1).map((t, i) => t - stamps[i]))}`
}
/** One Backspace in the editor per two frames, each step timed. */
const breakdown = async (n = 30) => {
  const v = view()
  const key = [], react = [], layout = [], paint = []
  for (let i = 0; i < n; i++) {
    await frame()
    await sleep(0)
    const t0 = performance.now()
    backspace(v.contentDOM)
    const t1 = performance.now()
    await null // React's update was queued first, so it has run by now
    const t2 = performance.now()
    void document.body.offsetHeight // style and layout, now
    const t3 = performance.now()
    await frame() // this frame is painted after its callbacks
    await frame()
    key.push(t1 - t0)
    react.push(t2 - t1)
    layout.push(t3 - t2)
    paint.push(performance.now() - t3)
  }
  return `median: key handler ${med(key)} ms, React ${med(react)} ms, style and layout ${med(layout)} ms, to the next painted frame ${med(paint)} ms`
}
const filler = (kb) => {
  const para = 'Ownership in Rust means each value has one owner, and the value is dropped when the owner goes out of scope. Borrowing lends access without moving it. '
  let s = ''
  while (s.length < kb * 1024) s += `## Section ${s.length}\n\n${para}${para}\n\n- a point about lifetimes\n- another point with \`code\`\n\n`
  return s
}
const fill = (v, text) => v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text }, selection: { anchor: text.length } })

;(async () => {
  try {
    const kb = typeof BODY_KB === 'number' ? BODY_KB : 20
    const page = (await invoke('list_pages', { filter: {} }))[0]
    await go(`/knowledge/${page.id}`)
    ;[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Edit')?.click()
    await sleep(1200)
    const v = view()
    fill(v, filler(kb))
    await sleep(1500)
    report(`page editor, ${kb} KB: ${await breakdown()}`)
    report(`page editor, ${kb} KB: ${await perFrame(() => backspace(v.contentDOM))}`)

    const title = document.querySelector('input[aria-label="Title"]')
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set
    set.call(title, title.value + ' with a much longer title to delete'.repeat(4))
    title.dispatchEvent(new InputEvent('input', { bubbles: true }))
    await sleep(600)
    const deleteInTitle = () => {
      set.call(title, title.value.slice(0, -1))
      title.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }))
    }
    report(`page title field: ${await perFrame(deleteInTitle, 60)}`)

    const task = (await invoke('list_tasks', { filter: {} }))[0]
    await go(`/tasks/${task.key}`)
    ;[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Edit')?.click()
    await sleep(1200)
    const t = view()
    for (const size of [1, kb, 80]) {
      fill(t, filler(size))
      await sleep(1500)
      report(`task description, ${size} KB: ${await perFrame(() => backspace(t.contentDOM))}`)
    }
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
