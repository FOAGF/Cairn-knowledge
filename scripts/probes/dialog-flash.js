// Dialog flash check: records the open dialog's opacity on every painted frame while Ctrl+K opens
// and closes, then the New task dialog opens and closes. Prints e.g. "ctrl-k close: 40-----".
// Per frame: '-' no dialog, 0-8 partly visible (opacity x 9), 'F' fully visible.
// A flash is an 'F' after the fade-out has started (e.g. "00F--"): the dialog pops back before it goes.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const frames = (ms) =>
    new Promise((done) => {
      const out = []
      const t0 = performance.now()
      const tick = () => {
        const d = document.querySelector('[role="dialog"]')
        const o = d ? parseFloat(getComputedStyle(d).opacity) : -1
        out.push(o < 0 ? '-' : o >= 0.995 ? 'F' : String(Math.round(o * 9)))
        if (performance.now() - t0 < ms) requestAnimationFrame(tick)
        else done(out.join(''))
      }
      requestAnimationFrame(tick)
    })
  const ctrlK = () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }))
  const escape = () => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))

  document.querySelector('a[href="/tasks"]')?.click()
  await sleep(1500)
  ctrlK(); report('ctrl-k open:  ' + (await frames(400)))
  ctrlK(); report('ctrl-k close: ' + (await frames(400)))
  const newTask = [...document.querySelectorAll('button')].find((b) => b.textContent.trim().endsWith('New task'))
  if (!newTask) return report('New task button not found')
  newTask.click(); report('new task open:  ' + (await frames(400)))
  escape(); report('new task close: ' + (await frames(400)))
})()
