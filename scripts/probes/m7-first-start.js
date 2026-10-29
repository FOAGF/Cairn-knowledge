// M7 part A probe: the first-start card on Home (FR-DATA-8). Needs a Vite started with
// VITE_CAIRN_NO_SEED=1 (no sample data) and a fresh CAIRN_DATA_DIR. Run with the page baked to /.
//   MODE 'fresh'   clicks Start fresh
//   MODE 'import'  imports IMPORT_FILE with the command while the card is shown (the native file
//                  dialog can't be driven from a probe), to check the card goes away
// Edit MODE below (the probe has no way to read the environment).
const MODE = 'fresh'
const IMPORT_FILE = '/tmp/claude-1000/cairn-m7-a/d1/backups/cairn-manual-2026-10-16-1931.zip'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const button = (root, re) => [...root.querySelectorAll('button')].find((b) => re.test(b.textContent))

;(async () => {
  try {
    await sleep(2500)
    report('card shown: ' + !!button(document, /Start fresh/))
    report('welcomeDone before: ' + (await invoke('get_settings')).welcomeDone)
    if (MODE === 'fresh') {
      button(document, /Start fresh/).click()
    } else {
      await invoke('import_data', { path: IMPORT_FILE })
    }
    await sleep(2500)
    report('card shown after: ' + !!button(document, /Start fresh/))
    report('welcomeDone after: ' + (await invoke('get_settings')).welcomeDone)
    report('tasks: ' + (await invoke('list_tasks', { filter: { includeClosed: true } })).length)
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e)
  }
})()
