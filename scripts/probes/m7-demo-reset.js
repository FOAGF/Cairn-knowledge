// M7 part A probe: Settings › Backup & data › Demo data › Reset (development builds). Run with the
// page baked to /settings/data. It changes the user name, clicks Reset… and Reset; the app then
// reloads, and the dev seed adds the samples again. Check the data folder afterwards (the user
// name is gone, the samples are back).
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const button = (root, re) => [...root.querySelectorAll('button')].find((b) => re.test(b.textContent.trim()))

;(async () => {
  try {
    await sleep(6000) // the dev seed
    const count = async () => (await invoke('list_tasks', { filter: { includeClosed: true } })).length
    report('tasks before: ' + (await count()))
    await invoke('update_settings', { patch: { userName: 'Zed' } })
    report('user name before: ' + (await invoke('get_settings')).userName)
    button(document, /^Reset…$/).click()
    await sleep(800)
    button(document.querySelector('[role="dialog"]'), /^Reset$/).click()
    await sleep(500)
    report('clicked Reset')
  } catch (e) {
    report('PROBE ERROR: ' + e)
  }
})()
