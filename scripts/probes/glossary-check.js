// Glossary check (UI-KN-8, D-38) on the dev sample data: Ctrl+K glossary hits, "Used in" paging,
// first-occurrence underlines, and the category scope filter. Creates and then deletes one entry.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const go = (url) => { history.pushState({}, '', url); dispatchEvent(new PopStateEvent('popstate')) }
  const invoke = window.__TAURI_INTERNALS__.invoke
  const text = (sel, re) => [...document.querySelectorAll(sel)].map((x) => x.textContent.trim()).find((t) => re.test(t)) ?? '-'

  // 1. Ctrl+K: a long form finds the glossary entry (FR-KN-GL-5).
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }))
  await sleep(500)
  const input = document.querySelector('[cmdk-input]')
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, 'coordinated universal time')
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await sleep(3000)
  const groups = [...document.querySelectorAll('[cmdk-group]')].map(
    (g) => (g.querySelector('[cmdk-group-heading]')?.textContent ?? '?') + ': ' +
      [...g.querySelectorAll('[cmdk-item]')].slice(0, 2).map((i) => i.textContent.trim().slice(0, 40)).join(' / '))
  report('ctrl-k "coordinated universal time" -> ' + groups.join(' | '))
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }))
  await sleep(500)

  // 2. "Used in" paging: 3 first, then 10 more at a time.
  const e = await invoke('create_glossary_entry', { input: { term: 'sensor', description: 'A measuring unit in the field.' } })
  go('/glossary?term=' + e.id)
  await sleep(2500)
  const moreBtn = () => [...document.querySelectorAll('button')].find((b) => /^Show \d+ more/.test(b.textContent.trim()))
  report('used in: ' + text('h2,h3,p,span', /^Used in/) + ' · ' + text('span', /^Showing/) + ' · button: ' + (moreBtn()?.textContent.trim() ?? 'none'))
  if (moreBtn()) {
    moreBtn().click()
    await sleep(1500)
    report('after Show more: ' + text('span', /^Showing/) + ' · button: ' + (moreBtn()?.textContent.trim() ?? 'none'))
  }
  await invoke('delete_glossary_entry', { id: e.id })

  // 3. Only the first occurrence of each term is underlined (D-38).
  go('/knowledge/k-tz')
  await sleep(2500)
  const body = document.querySelector('.markdown-body')?.textContent ?? ''
  const marks = [...document.querySelectorAll('.markdown-body .decoration-dotted')].map((m) => m.textContent)
  report('underlined on Timezone handling: ' + JSON.stringify(marks) + ' · "UTC" appears ' + (body.match(/UTC/g) ?? []).length + 'x in the text')

  // 4. Scope filter: a category shows its entries plus the global ones that apply (D-38).
  go('/glossary')
  await sleep(2000)
  document.querySelector('[aria-label="Filter by scope"]')?.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }))
  await sleep(600)
  const opt = [...document.querySelectorAll('[role="option"]')].find((o) => o.textContent.includes('Sensor Platform'))
  if (!opt) return report('scope: option not found')
  // Radix Select picks an item on pointer up after a pointer down on that item.
  opt.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }))
  opt.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, button: 0, pointerType: 'mouse' }))
  await sleep(1000)
  const rows = [...document.querySelectorAll('button.flex-col')].map((b) => b.textContent.trim().replace(/\s+/g, ' '))
  report('scope "' + document.querySelector('[aria-label="Filter by scope"]')?.textContent.trim() + '" -> ' + rows.length + ' entries: ' + rows.join(' ; '))
})().catch((err) => report('ERROR ' + err))
