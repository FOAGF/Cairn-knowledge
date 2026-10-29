// D-72 probe, scenario C: switching both ways from the demo data (FR-KN-EXT-8, -9). Run on a fresh
// internal data folder (the default seed), with the folder to switch to prepended:
//   { echo "const ROOT = '/tmp/x-notes'"; cat scripts/probes/ext-switch.js; } > /tmp/p.js
//   CAIRN_DATA_DIR=/tmp/x ROOT=/tmp/x-notes SHOTS=/tmp/shots PROBE_AFTER=10 PROBE_EXIT=120 \
//     PROBE_DRIVER=scripts/probes/ext-switch-driver.sh scripts/probes/ext-probe.sh /tmp/p.js
// Internal → the folder (progress dialog, pages from the folder, a switch backup, tasks unchanged),
// then back to the database from that backup (the demo pages and their task links again, tasks
// unchanged), the switch-backups list with *Delete*, and a tiny warning size for the next start
// (ext-switch-warning.js checks the pop-up).
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
const $$ = (id) => [...document.querySelectorAll(`[data-testid="${id}"]`)]
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1000)
}
const pagesNow = async () => (await invoke('list_pages', { filter: { includeArchived: true } })).map((p) => p.title).sort()
const tasksNow = async () => (await invoke('list_tasks', { filter: {} })).map((t) => `${t.key}:${t.title}:${t.status}:${t.categoryId}`).sort().join('|')
const linksNow = async () => {
  let n = 0
  for (const t of await invoke('list_tasks', { filter: {} })) n += (await invoke('list_task_page_links', { taskId: t.id, pageId: null })).length
  return n
}
/** Follows the progress dialog until its result shows; returns the steps seen. */
const follow = async (shotWord) => {
  const steps = []
  let shot = false
  for (let i = 0; i < 600; i++) {
    const s = text($('knowledge-progress-step'))
    if ($('knowledge-progress-dialog') && steps[steps.length - 1] !== s) steps.push(s)
    if ($('knowledge-progress-dialog') && !shot) {
      report('@@' + shotWord)
      shot = true
    }
    if ($('knowledge-progress-result') || $('knowledge-progress-error')) break
    await sleep(50)
  }
  return steps
}

;(async () => {
  try {
    let pages0 = []
    for (let i = 0; i < 40 && pages0.length === 0; i++) {
      pages0 = await pagesNow()
      await sleep(500)
    }
    await sleep(2000)
    pages0 = await pagesNow()
    const tasks0 = await tasksNow()
    const someTask = (await invoke('list_tasks', { filter: {} }))[0]
    const somePage = (await invoke('list_pages', { filter: {} }))[0]
    await invoke('link_task_page', { taskId: someTask.id, pageId: somePage.id }).catch(() => {})
    const links0 = await linksNow()
    const glossary0 = (await invoke('list_glossary')).length
    report(`C0 internal: ${pages0.length} pages, ${tasks0.split('|').length} tasks, ${links0} task↔page links, glossary ${glossary0}; mode=${(await invoke('get_knowledge_storage')).mode}`)

    // Internal → external.
    await go('/settings/general')
    window.__cairnPickFolder = () => ROOT
    $('storage-switch-external').click()
    await sleep(800)
    report('C1 dialog: ' + text($('switch-external-dialog')).slice(0, 160))
    $('switch-confirm').click()
    const steps = await follow('shot-progress')
    report('C1 progress steps: ' + steps.join(' → '))
    report('C1 result: ' + text($('knowledge-progress-result')) + ' ' + text($('knowledge-progress-error')))
    report('@@shot-progress-done')
    await sleep(2500)
    $('knowledge-progress-close')?.click()
    await sleep(1000)
    const ext = await invoke('list_pages', { filter: { includeArchived: true } })
    const bk1 = await invoke('list_switch_backups')
    report(`C1 after: mode=${JSON.stringify(await invoke('get_knowledge_storage'))}`)
    report(`C1 pages from the folder: ${ext.map((p) => p.path).sort().join(', ')}`)
    report(`C1 tasks unchanged=${(await tasksNow()) === tasks0}, task↔page links now ${await linksNow()}, glossary ${(await invoke('list_glossary')).length}`)
    report(`C1 switch backups: ${bk1.backups.map((b) => `${b.fileName} (${b.mode}, ${b.pages} pages, ${b.size} B)`).join('; ')} in ${bk1.folder}`)
    report('@@list-backups')
    await sleep(1000)

    // External → internal, from that backup.
    await go('/settings/general')
    $('storage-switch-internal').click()
    await sleep(800)
    report('C2 choices: ' + $$('switch-backup-choice').map((l) => `${text(l)} [${l.querySelector('[role="radio"]')?.getAttribute('aria-checked')}]`).join(' | ') + ' | ' + text($('switch-start-empty')))
    $('switch-confirm').click()
    const steps2 = await follow('shot-progress-back')
    report('C2 progress steps: ' + steps2.join(' → '))
    report('C2 result: ' + text($('knowledge-progress-result')) + ' ' + text($('knowledge-progress-error')))
    $('knowledge-progress-close')?.click()
    await sleep(1000)
    const pages2 = await pagesNow()
    report(`C2 after: mode=${(await invoke('get_knowledge_storage')).mode}; demo pages back=${JSON.stringify(pages2) === JSON.stringify(pages0)} (${pages2.length} of ${pages0.length}); tasks unchanged=${(await tasksNow()) === tasks0}; task↔page links ${await linksNow()} (before ${links0})`)
    if (JSON.stringify(pages2) !== JSON.stringify(pages0)) report('C2 differ: missing ' + pages0.filter((t) => !pages2.includes(t)).join(', ') + ' | extra ' + pages2.filter((t) => !pages0.includes(t)).join(', '))

    // The list, and Delete.
    await go('/settings/data')
    await sleep(500)
    report('C3 rows: ' + $$('switch-backup-row').map(text).join(' | ') + ' | ' + text($('switch-backups-total')))
    report('@@copy-backups')
    await sleep(1500)
    const row = $$('switch-backup-row').find((r) => text(r).includes('folder'))
    ;[...(row ?? $$('switch-backup-row')[0]).querySelectorAll('button')].find((b) => text(b) === 'Delete').click()
    await sleep(600)
    report('C3 delete dialog: ' + text($('switch-backup-delete-dialog')).slice(0, 200))
    $('switch-backup-delete-confirm').click()
    await sleep(1200)
    report('C3 after delete: ' + $$('switch-backup-row').map(text).join(' | ') + ' | ' + text($('switch-backups-total')))
    report('@@list-backups')
    await sleep(1000)
    const s = await invoke('update_settings', { patch: { switchBackupWarnGb: 0.000001 } })
    report('C4 warning size set: ' + s.switchBackupWarnGb + ' GB; over now=' + (await invoke('list_switch_backups')).overWarning)
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
