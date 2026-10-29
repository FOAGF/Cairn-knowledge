// D-71 probe, after pending-edit.js: the waiting page comes back after a restart (stage 2), and
// with no record of it the newest page with unsaved changes takes the tab (stage 3, D-71's Q6a).
// Prepend the ids pending-edit.js reported and the stage:
//   { echo 'const IDS = {...}; const STAGE = 2'; cat scripts/probes/pending-edit-restart.js; } > /tmp/p.js
//   CAIRN_DATA_DIR=/tmp/x PROBE_AFTER=8 PROBE_EXIT=40 scripts/ui-probe.sh /tmp/p.js
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
const tabState = () => (tab() ? `"${text(tab())}" → ${tab().getAttribute('href')}` : 'no tab')
const editing = () => !!document.querySelector('[role="toolbar"][aria-label="Editing"]')
const body = () => document.querySelector('.cm-content')?.cmTile?.root?.view?.state.doc.toString() ?? '(no editor)'
const prompt = () => document.querySelector('[role="status"]')
const dialog = () => document.querySelector('[role="dialog"]')
const answer = async (t) => {
  ;[...dialog().querySelectorAll('button')].find((x) => text(x) === t).click()
  await sleep(900)
}
const draftOf = async (id) => invoke('get_draft', { kind: 'page', refId: id })

;(async () => {
  try {
    const { p1, p2, p4 } = IDS
    if (STAGE === 2) {
      report('2 at start: ' + tabState() + ' (p4=' + (tab()?.getAttribute('href') === '/knowledge/' + p4) + ')')
      tab().click()
      await sleep(1500)
      report('2 via tab: editing=' + editing() + ', has change=' + body().includes('PROBE-RESTART'))
      await go('/knowledge/' + p2)
      await sleep(500)
      report('2 p2: restore prompt=' + text(prompt()))
      button('Restore').click()
      await sleep(600)
      report('2 restore asks: ' + text(dialog()?.querySelector('p')))
      await answer('Continue editing')
      report('2 continue: on p4=' + location.pathname.endsWith(p4) + ', editing=' + editing())
      await go('/knowledge/' + p2)
      await sleep(500)
      button('Restore').click()
      await sleep(600)
      await answer('Discard them and edit this page')
      report('2 restored p2: editing=' + editing() + ', has old change=' + body().includes('PROBE-OLD') + ', ' + tabState() + ', p4 draft=' + !!(await draftOf(p4)))
      // For stage 3: newer changes on p1, and no record of the waiting page.
      await go('/tasks')
      const page1 = await invoke('get_page', { id: p1 })
      await sleep(1200)
      await invoke('save_draft', { kind: 'page', refId: p1, content: JSON.stringify({ title: 'Adopted title', body: page1.body + '\n\nPROBE-ADOPT', tags: page1.tags }) })
      await invoke('delete_draft', { kind: 'new_page', refId: 'pending' })
      report('2 left: p1 and p2 have changes, no record')
    } else {
      report('3 at start: ' + tabState() + ' (p1=' + (tab()?.getAttribute('href') === '/knowledge/' + p1) + ')')
      await go('/knowledge/' + p2)
      await sleep(500)
      report('3 p2: editing=' + editing() + ', restore prompt=' + !!prompt())
      tab().click()
      await sleep(1500)
      report('3 via tab: editing=' + editing() + ', has change=' + body().includes('PROBE-ADOPT') + ', title field=' + document.querySelector('input[aria-label="Title"]')?.value)
    }
    report('DONE')
  } catch (e) {
    report('PROBE ERROR: ' + e + ' ' + e.stack)
  }
})()
