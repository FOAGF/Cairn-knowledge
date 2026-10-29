// M7 part A probe: Settings › Backup & data. Run with the page baked to /settings/data:
//   CAIRN_DATA_DIR=/tmp/x PROBE_PORT=5291 PROBE_EXIT=110 scripts/ui-probe.sh scripts/probes/m7-backups.js
// It makes a manual backup, switches to a custom folder (BACKUP_DIR below), waits for the daily
// backup to go there, then restores a backup through the dialog.
const BACKUP_DIR = '/tmp/claude-1000/cairn-m7-a/custom-folder'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const button = (text) => [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === text)
const rows = () => [...document.querySelectorAll('ul[aria-label="Backups"] li')].map((li) => li.textContent.replace(/\s+/g, ' ').trim())

;(async () => {
  try {
    await sleep(2500)
    report('start: ' + JSON.stringify(rows()))

    button('Back up now').click()
    await sleep(2500)
    report('after Back up now: ' + JSON.stringify(rows()))

    // A relative path is refused; a full path is saved.
    try {
      await invoke('set_backup_folder', { folder: 'relative/dir' })
      report('relative path: NOT refused')
    } catch (e) {
      report('relative path refused: ' + e.message)
    }
    await invoke('set_backup_folder', { folder: BACKUP_DIR })
    await sleep(1500)
    report('custom folder set, buttons: ' + [...document.querySelectorAll('button')].map((b) => b.textContent.trim()).filter((t) => /folder/i.test(t)).join(' | '))
    report('list in the new (empty) folder: ' + JSON.stringify(rows()))

    // The thread notices the folder change within a minute and makes today's backup there.
    for (let i = 0; i < 70 && rows().length === 0; i++) await sleep(1000)
    report('after the daily backup in the custom folder: ' + JSON.stringify(rows()))
    report('status: ' + JSON.stringify(await invoke('backup_status')))

    // Back to the default folder, then restore the newest backup there.
    button('Use default folder').click()
    await sleep(1500)
    report('default folder again: ' + JSON.stringify(rows()))
    button('Restore…').click()
    await sleep(800)
    const dlg = document.querySelector('[role="dialog"]')
    report('dialog: ' + dlg.textContent.replace(/\s+/g, ' ').trim())
    ;[...dlg.querySelectorAll('button')].find((b) => /Back up and replace/.test(b.textContent)).click()
    await sleep(6000)
    const dlg2 = document.querySelector('[role="dialog"]')
    report('after restore: ' + (dlg2 ? dlg2.textContent.replace(/\s+/g, ' ').trim() : 'no dialog'))
    report('list after restore: ' + JSON.stringify(rows()))
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e)
  }
})()
