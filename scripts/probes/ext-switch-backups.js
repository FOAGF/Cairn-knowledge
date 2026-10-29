// D-72 probe: switch backups hold knowledge only, and each external folder keeps only its newest one
// (internal-mode ones are all kept); importing a switch backup is refused in the import dialog.
// Prepend the folder, then run with its driver (which copies a switch backup into the backups folder):
//   { echo "const FOLDER_A = '/tmp/x/a'"; cat scripts/probes/ext-switch-backups.js; } > /tmp/p.js
//   CAIRN_DEV_KNOWLEDGE_MODE=external=/tmp/x/a CAIRN_DATA_DIR=/tmp/x/data PROBE_AFTER=10 PROBE_EXIT=120 \
//     PROBE_DRIVER=scripts/probes/ext-switch-backups-driver.sh scripts/probes/ext-probe.sh /tmp/p.js CAIRN_DEV_TRASH_DIR=/tmp/x/trash
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(900)
}
const backups = async () => (await invoke('list_switch_backups')).backups.map((b) => `${b.mode} ${b.pages}p${b.knowledgeOnly ? ' knowledge-only' : ' FULL'}${b.root ? ' root=' + b.root.split('/').pop() : ''}`)
/** Starts a switch and waits for the progress event that finishes it (the dialog shows it). */
const switchAndWait = async (cmd, args) => {
  await go('/settings/general')
  await invoke(cmd, args)
  for (let i = 0; i < 120 && !$('knowledge-progress-close'); i++) await sleep(250)
  const out = text($('knowledge-progress-dialog')?.querySelector('h2'))
  $('knowledge-progress-close')?.click()
  await sleep(600)
  return out
}

;(async () => {
  try {
    report('start: ' + (await invoke('get_knowledge_storage')).mode + ', ' + (await invoke('list_pages', { filter: {} })).length + ' pages | backups: ' + (await backups()).join(' ; '))
    report('A→internal: ' + (await switchAndWait('switch_knowledge_to_internal', { backup: null })) + ' | backups: ' + (await backups()).join(' ; '))
    report('internal→A: ' + (await switchAndWait('switch_knowledge_to_external', { root: FOLDER_A })) + ' | pages ' + (await invoke('list_pages', { filter: {} })).length + ' | backups: ' + (await backups()).join(' ; '))
    report('A→internal again: ' + (await switchAndWait('switch_knowledge_to_internal', { backup: null })) + ' | backups: ' + (await backups()).join(' ; '))

    // Import refusal: the driver copies the external switch backup into the backups folder.
    report('@@copy')
    await sleep(2500)
    await go('/settings/data')
    await sleep(800)
    const row = [...document.querySelectorAll('li, [role="row"], div')]
      .filter((el) => text(el).includes('cairn-manual-') && [...el.querySelectorAll('button')].some((b) => text(b).startsWith('Restore')))
      .sort((a, b) => text(a).length - text(b).length)[0]
    ;[...row.querySelectorAll('button')].find((b) => text(b).startsWith('Restore')).click()
    await sleep(1500)
    report('import dialog: ' + text($('import-dialog')).slice(0, 300) + ' | confirm button shown=' + !!$('import-confirm'))
    report('@@shot')
    await sleep(2000)
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
