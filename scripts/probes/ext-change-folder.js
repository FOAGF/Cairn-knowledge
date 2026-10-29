// D-72 probe: changing the knowledge folder while in external mode (Settings › General ›
// *Change folder…*): one switch with one switch backup, as if switching to internal mode and on to the
// other folder. Prepend the two folders (copies; the probe doesn't write into them):
//   { echo "const FOLDER_A = '/tmp/x/a', FOLDER_B = '/tmp/x/b'"; cat scripts/probes/ext-change-folder.js; } > /tmp/p.js
//   CAIRN_DATA_DIR=/tmp/x/data PROBE_AFTER=10 PROBE_EXIT=90 scripts/ui-probe.sh /tmp/p.js \
//     CAIRN_DEV_KNOWLEDGE_MODE=external=/tmp/x/a CAIRN_DEV_TRASH_DIR=/tmp/x/trash
// Marks a page of A "not checked" (Cairn-only data), changes to B through the dialog, checks the index
// and the switch backup, tries the folder in use, then changes back to A and checks the mark came back.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(900)
}
const pages = async () => invoke('list_pages', { filter: { includeArchived: true } })
/** Waits for the switch's progress dialog to finish; returns its title and result text, then closes it. */
const finish = async () => {
  for (let i = 0; i < 120 && !$('knowledge-progress-close'); i++) await sleep(250)
  const out = text($('knowledge-progress-dialog')?.querySelector('h2')) + ' | ' + text($('knowledge-progress-result') ?? $('knowledge-progress-error'))
  $('knowledge-progress-close')?.click()
  await sleep(600)
  return out
}
/** *Change folder…* with `folder` picked; returns the dialog's text (without pressing anything). */
const openChange = async (folder) => {
  await go('/settings/general')
  window.__cairnPickFolder = () => folder
  $('storage-change-folder').click()
  await sleep(700)
  return text($('switch-folder-dialog'))
}

;(async () => {
  try {
    let all = await pages()
    report('start: ' + all.length + ' pages | root ' + (await invoke('get_knowledge_storage')).root)
    const marked = all.find((p) => p.title === 'Rust ownership')
    await invoke('set_page_check_status', { id: marked.id, status: 'not_checked' })
    report('marked "Rust ownership" (' + marked.path + ') not checked')

    // A → B through the dialog.
    report('dialog: ' + (await openChange(FOLDER_B)).slice(0, 400))
    $('switch-confirm').click()
    report('A→B: ' + (await finish()))
    all = await pages()
    report('after A→B: ' + all.length + ' pages: ' + all.map((p) => p.path).join(', ') + ' | root ' + (await invoke('get_knowledge_storage')).root)
    let backups = (await invoke('list_switch_backups')).backups
    report('switch backups: ' + backups.map((b) => `${b.mode} ${b.pages}p root=${b.root}`).join(' ; '))

    // The folder in use is refused, in the dialog.
    await openChange(FOLDER_B)
    $('switch-confirm').click()
    await sleep(900)
    report('same folder: ' + text($('switch-folder-dialog')?.querySelector('[role="alert"]')) + ' | backups still ' + (await invoke('list_switch_backups')).backups.length)
    ;[...$('switch-folder-dialog').querySelectorAll('button')].find((b) => text(b) === 'Cancel').click()
    await sleep(500)

    // B → A: A's Cairn-only data comes back by path.
    await openChange(FOLDER_A)
    $('switch-confirm').click()
    report('B→A: ' + (await finish()))
    all = await pages()
    const back = all.find((p) => p.path === marked.path)
    report('after B→A: ' + all.length + ' pages | "Rust ownership" id same=' + (back?.id === marked.id) + ' check=' + back?.checkStatus)
    backups = (await invoke('list_switch_backups')).backups
    report('switch backups: ' + backups.map((b) => `${b.mode} ${b.pages}p root=${b.root}`).join(' ; '))
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
