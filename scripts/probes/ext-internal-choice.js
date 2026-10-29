// D-72 probe: going internal offers and accepts only switch backups made in internal mode. Prepend
// the two folders (copies), then run on a fresh internal data folder:
//   { echo "const FOLDER_A = '/tmp/x/a', FOLDER_B = '/tmp/x/b'"; cat scripts/probes/ext-internal-choice.js; } > /tmp/p.js
//   CAIRN_DATA_DIR=/tmp/x/data PROBE_AFTER=10 PROBE_EXIT=90 scripts/ui-probe.sh /tmp/p.js CAIRN_DEV_TRASH_DIR=/tmp/x/trash
// Internal → A (an internal backup), A → B (A's external backup), then *Switch to internal mode…*
// lists only the internal one, and asking for A's backup directly is refused with nothing changed.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(900)
}
const switchAndWait = async (cmd, args) => {
  await go('/settings/general')
  await invoke(cmd, args)
  for (let i = 0; i < 120 && !$('knowledge-progress-close'); i++) await sleep(250)
  $('knowledge-progress-close')?.click()
  await sleep(600)
}
const list = async () => (await invoke('list_switch_backups')).backups

;(async () => {
  try {
    await switchAndWait('switch_knowledge_to_external', { root: FOLDER_A })
    await switchAndWait('switch_knowledge_folder', { root: FOLDER_B })
    const backups = await list()
    report('backups: ' + backups.map((b) => `${b.mode} ${b.pages}p`).join(' ; '))

    await go('/settings/general')
    $('storage-switch-internal').click()
    await sleep(800)
    report('dialog choices: ' + [...document.querySelectorAll('[data-testid="switch-backup-choice"]')].map(text).join(' | ') + ' || ' + text($('switch-start-empty')))
    ;[...$('switch-internal-dialog').querySelectorAll('button')].find((b) => text(b) === 'Cancel').click()
    await sleep(500)

    const external = backups.find((b) => b.mode === 'external')
    const pagesBefore = (await invoke('list_pages', { filter: { includeArchived: true } })).length
    let refused = 'not refused'
    try {
      await invoke('switch_knowledge_to_internal', { backup: external.fileName })
    } catch (e) {
      refused = `${e.kind}: ${e.message}`
    }
    await sleep(1500)
    const s = await invoke('get_knowledge_storage')
    report('direct request: ' + refused + ' | mode ' + s.mode + ', pages ' + (await invoke('list_pages', { filter: { includeArchived: true } })).length + ' (before ' + pagesBefore + '), backups ' + (await list()).length)
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
