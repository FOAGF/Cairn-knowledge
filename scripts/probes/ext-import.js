// D-72 probe, scenario D: importing an external-mode backup whose folder isn't there, without the
// folder (FR-KN-EXT-14). Two stages on two data folders; prepend `const STAGE = 'make'` or 'import':
// - make (external mode, e.g. CAIRN_DEV_KNOWLEDGE_MODE=external=/tmp/x-notes): adds a category, a
//   task, a glossary entry, a conversation and a user name that only the backup will have, then
//   *Back up now*; reports the zip. Copy it into the other data folder's `backups/` and move the
//   knowledge folder away before stage 2.
// - import (a fresh internal data folder with the demo data): sets a user name and adds a provider
//   of this computer, then *Restore…* of that zip from the Backup card: the folder question, a
//   cancelled folder choice, *Import without the folder…*, the warning, *Back up and import tasks*.
//   Reports what the data says afterwards.
//   { echo "const STAGE = 'import'"; cat scripts/probes/ext-import.js; } > /tmp/p.js
//   CAIRN_DATA_DIR=/tmp/y PROBE_AFTER=10 PROBE_EXIT=60 scripts/probes/ext-probe.sh /tmp/p.js
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(1000)
}
const counts = async () => {
  const tasks = await invoke('list_tasks', { filter: {} })
  return {
    mode: (await invoke('get_knowledge_storage')).mode,
    userName: (await invoke('get_settings')).userName,
    providers: (await invoke('list_providers')).map((p) => p.name),
    categories: (await invoke('list_categories', { includeArchived: true })).map((c) => c.name),
    tasks: tasks.length,
    markerTask: tasks.filter((t) => t.title.includes('external backup') || t.title.includes('internal only')).map((t) => t.key + ' ' + t.title),
    pages: (await invoke('list_pages', { filter: { includeArchived: true } })).length,
    glossary: (await invoke('list_glossary')).length,
    conversations: (await invoke('list_conversations')).length,
  }
}

;(async () => {
  try {
    for (let i = 0; i < 30 && (await invoke('list_tasks', { filter: {} })).length === 0; i++) await sleep(500)
    await sleep(4000)
    if (STAGE === 'make') {
      const cat = await invoke('create_category', { input: { name: 'Field notes', key: 'FLD', color: '#0d9488' } })
      await invoke('create_task', { input: { categoryId: cat.id, title: 'Task from the external backup', description: 'Only in the backup.' } })
      await invoke('create_glossary_entry', { input: { term: 'EXTGLOSS', description: 'Only in the backup.' } })
      await invoke('create_conversation', { input: { title: 'Conversation in the backup' } })
      await invoke('update_settings', { patch: { userName: 'External User' } })
      report('make: ' + JSON.stringify(await counts()))
      const b = await invoke('backup_now')
      report('BACKUP ' + b.path)
      report('make: backup info ' + JSON.stringify(await invoke('read_backup_info', { path: b.path })))
      report('DONE')
      return
    }
    // STAGE === 'import'
    await invoke('update_settings', { patch: { userName: 'Internal User' } })
    await invoke('create_provider', { input: { name: 'Probe provider', kind: 'openai_compatible', endpoint: 'http://127.0.0.1:5298/v1', auth: 'none' } })
    const general = (await invoke('list_categories', { includeArchived: false })).find((c) => c.name === 'General')
    await invoke('create_task', { input: { categoryId: general.id, title: 'Task internal only' } })
    report('before: ' + JSON.stringify(await counts()))
    await go('/settings/data')
    await sleep(800)
    const row = [...document.querySelectorAll('li, [role="row"], div')].filter((el) => text(el).includes('cairn-manual-') && [...el.querySelectorAll('button')].some((b) => text(b).startsWith('Restore'))).sort((a, b) => text(a).length - text(b).length)[0]
    report('backup row: ' + text(row).slice(0, 160))
    ;[...row.querySelectorAll('button')].find((b) => text(b).startsWith('Restore')).click()
    await sleep(1500)
    report('D1 dialog: ' + text($('import-dialog')).slice(0, 400))
    window.__cairnPickFolder = () => null
    $('import-choose-root')?.click()
    await sleep(800)
    report('D2 after a cancelled folder choice: still asking=' + !!$('import-without-root') + ', import-root=' + text($('import-root')))
    $('import-without-root').click()
    await sleep(800)
    report('D3 warning: ' + text($('import-tasks-only-warning')) + ' | button "' + text($('import-confirm')) + '"')
    $('import-confirm').click()
    for (let i = 0; i < 60 && !text($('import-dialog')).includes('Import finished'); i++) await sleep(250)
    report('D4 result: ' + text($('import-dialog')).slice(0, 400))
    await sleep(1500)
    report('after: ' + JSON.stringify(await counts()))
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
