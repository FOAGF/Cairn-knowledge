// D-71 probe: a saved page with unsaved changes is the pending page (UI-KN-6a). Run with the demo
// data; it opens the pages itself:
//   CAIRN_DATA_DIR=/tmp/x PROBE_AFTER=8 PROBE_EXIT=75 scripts/ui-probe.sh scripts/probes/pending-edit.js
// Edits a page (the tab appears at the first change), leaves and comes back through the tab, is asked
// before editing a second page (continue, then discard), is asked before a new page, saves, cancels,
// and deletes a page that has changes. It ends with page 4 changed and waiting, for
// pending-edit-restart.js to find after a restart.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args)
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const go = async (path) => {
  history.pushState({}, '', path)
  dispatchEvent(new PopStateEvent('popstate'))
  await sleep(900)
}
const button = (t) => [...document.querySelectorAll('button')].find((b) => text(b) === t)
const tab = () => [...document.querySelectorAll('header nav a')].find((a) => a.querySelector('[aria-label="Not saved"]'))
const tabState = () => (tab() ? `"${text(tab())}" → ${tab().getAttribute('href')}${tab().className.includes('bg-accent') ? ' (highlighted)' : ''}` : 'no tab')
const knowledgeLit = () => [...document.querySelectorAll('header nav a')].find((a) => text(a) === 'Knowledge').className.includes('bg-accent')
const editing = () => !!document.querySelector('[role="toolbar"][aria-label="Editing"]')
const view = () => document.querySelector('.cm-content')?.cmTile?.root?.view
const typeAtEnd = (s) => {
  const v = view()
  v.dispatch({ changes: { from: v.state.doc.length, insert: s }, selection: { anchor: v.state.doc.length + s.length } })
}
const body = () => view()?.state.doc.toString() ?? '(no editor)'
const dialog = () => document.querySelector('[role="dialog"]')
const answer = async (t) => {
  const b = [...(dialog()?.querySelectorAll('button') ?? [])].find((x) => text(x) === t)
  if (!b) throw new Error('no button "' + t + '" in ' + text(dialog()))
  b.click()
  await sleep(900)
}
const draftOf = async (id) => invoke('get_draft', { kind: 'page', refId: id })
const slot = async () => (await invoke('get_draft', { kind: 'new_page', refId: 'pending' }))?.content ?? 'none'

;(async () => {
  try {
    const pages = (await invoke('list_pages', { filter: {} })).filter((p) => !p.archived)
    const [p1, p2, p3, p4, p5] = pages
    report('pages: ' + [p1, p2, p3, p4, p5].map((p) => p.title).join(' | '))

    // 1. Edit: no tab until the first change, then a highlighted tab and Knowledge not lit.
    await go('/knowledge/' + p1.id)
    button('Edit').click()
    await sleep(500)
    report('1 after Edit: editing=' + editing() + ', ' + tabState())
    typeAtEnd('\n\nPROBE-ONE')
    await sleep(300)
    report('1 after typing: ' + tabState() + ', Knowledge lit=' + knowledgeLit())

    // 2. Leave: the tab stays; the changes reach the database.
    await go('/tasks')
    await sleep(3500)
    report('2 on /tasks: ' + tabState() + ' | page draft saved=' + !!(await draftOf(p1.id)) + ' | slot=' + (await slot()))

    // 3. Back through the tab: straight into editing with the change, no restore prompt.
    tab().click()
    await sleep(1200)
    report('3 via tab: url=' + location.pathname + ', editing=' + editing() + ', has change=' + body().includes('PROBE-ONE') + ', restore prompt=' + !!document.querySelector('[role="status"]') + ', ' + tabState())

    // 3b. Through the tree instead (from another page): the same.
    await go('/knowledge/' + p2.id)
    await go('/knowledge/' + p1.id)
    await sleep(300)
    report('3b via route: editing=' + editing() + ', has change=' + body().includes('PROBE-ONE'))

    // 4. Edit a second page: asked; Continue editing goes back to the first.
    await go('/knowledge/' + p2.id)
    button('Edit').click()
    await sleep(600)
    report('4 ask: ' + text(dialog()?.querySelector('p')) + ' | buttons: ' + [...(dialog()?.querySelectorAll('button') ?? [])].map(text).join(' / '))
    await answer('Continue editing')
    await sleep(400)
    report('4 continue: url=' + location.pathname + ' (p1=' + (location.pathname.endsWith(p1.id)) + '), editing=' + editing() + ', has change=' + body().includes('PROBE-ONE'))

    // 5. Again, and discard: the first page's changes are gone; the second page edits; its first change takes the tab.
    await go('/knowledge/' + p2.id)
    button('Edit').click()
    await sleep(600)
    await answer('Discard them and edit this page')
    report('5 discard: editing p2=' + editing() + ', ' + tabState() + ', p1 draft=' + !!(await draftOf(p1.id)))
    typeAtEnd('\n\nPROBE-TWO')
    await sleep(300)
    report('5 typed on p2: ' + tabState())
    await go('/knowledge/' + p1.id)
    report('5 p1 now: editing=' + editing() + ', restore prompt=' + !!document.querySelector('[role="status"]'))

    // 6. A new page while p2 waits: asked; continue, then discard and start it.
    await go('/knowledge/new')
    ;[...document.querySelectorAll('h2')].find((h) => text(h) === 'Write manually').closest('button').click()
    await sleep(700)
    report('6 ask: ' + text(dialog()?.querySelector('p')))
    await answer('Continue editing')
    report('6 continue: on p2=' + location.pathname.endsWith(p2.id) + ', editing=' + editing() + ', has change=' + body().includes('PROBE-TWO'))
    await go('/knowledge/new')
    ;[...document.querySelectorAll('h2')].find((h) => text(h) === 'Write manually').closest('button').click()
    await sleep(700)
    await answer('Discard them and start the new one')
    await sleep(400)
    report('6 discard: url=' + location.pathname + location.search + ', ' + tabState() + ', p2 draft=' + !!(await draftOf(p2.id)))

    // 7. Edit p3 while the new page waits: asked about the new page; discard it, type, Save.
    await go('/knowledge/' + p3.id)
    button('Edit').click()
    await sleep(600)
    report('7 ask: ' + text(dialog()?.querySelector('p')))
    await answer('Discard it and edit this page')
    typeAtEnd('\n\nPROBE-SAVED')
    await sleep(300)
    report('7 typed: ' + tabState())
    button('Save').click()
    await sleep(1500)
    report('7 saved: editing=' + editing() + ', ' + tabState() + ', body has it=' + (await invoke('get_page', { id: p3.id })).body.includes('PROBE-SAVED') + ', draft=' + !!(await draftOf(p3.id)))

    // 8. Cancel drops the tab and the changes.
    button('Edit').click()
    await sleep(500)
    typeAtEnd('\n\nPROBE-CANCEL')
    await sleep(300)
    const before = tabState()
    button('Cancel').click()
    await sleep(800)
    report('8 cancel: before ' + before + ' → after ' + tabState() + ', draft=' + !!(await draftOf(p3.id)))

    // 9. A page with changes is deleted elsewhere: the tab goes, with a notice.
    await go('/knowledge/' + p5.id)
    button('Edit').click()
    await sleep(500)
    typeAtEnd('\n\nPROBE-DELETED')
    await sleep(300)
    await go('/tasks')
    report('9 before delete: ' + tabState())
    await invoke('delete_page', { id: p5.id })
    await sleep(1500)
    report('9 after delete: ' + tabState() + ' | toast: ' + text(document.querySelector('[data-sonner-toast]')))

    // 10. Leave p4 changed and waiting, plus older changes on p2 kept for its restore prompt.
    await go('/knowledge/' + p4.id)
    button('Edit').click()
    await sleep(500)
    typeAtEnd('\n\nPROBE-RESTART')
    await sleep(300)
    await go('/tasks')
    await invoke('save_draft', { kind: 'page', refId: p2.id, content: JSON.stringify({ title: p2.title, body: p2.body + '\n\nPROBE-OLD', tags: p2.tags }) })
    await sleep(3500)
    report('10 waiting: ' + tabState() + ' | slot=' + (await slot()) + ' | p4 draft=' + !!(await draftOf(p4.id)))
    report('IDS ' + JSON.stringify({ p1: p1.id, p2: p2.id, p4: p4.id }))
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
