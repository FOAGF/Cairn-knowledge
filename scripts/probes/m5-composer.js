// M5 chat through the real composer (FR-KN-UP-7): attach a file with an instruction, send, and sample
// the assistant message while the draft streams in. Needs the mock LLM and `m5-flows.js` run first
// (it sets up the model); prepend the samples with `with-samples.sh`.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const invoke = window.__TAURI_INTERNALS__.invoke
  const go = (url) => { history.pushState({}, '', url); dispatchEvent(new PopStateEvent('popstate')) }
  const bytes = (b64) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))
  go('/assistant')
  await sleep(2000)
  const newBtn = [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'New conversation')
  newBtn?.click()
  await sleep(1000)
  const input = document.querySelector('[data-testid="attach-input"]')
  const dt = new DataTransfer()
  dt.items.add(new File([bytes(window.M5_SAMPLES['pump-manual.pdf'])], 'pump-manual.pdf', { type: 'application/pdf' }))
  input.files = dt.files
  input.dispatchEvent(new Event('change', { bubbles: true }))
  await sleep(500)
  const box = document.querySelector('textarea')
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(box, 'Only the calibration part')
  box.dispatchEvent(new Event('input', { bubbles: true }))
  await sleep(300)
  document.querySelector('[aria-label="Send"]')?.click()
  // Sample the newest assistant message as it grows.
  const seen = []
  for (let i = 0; i < 40; i++) {
    await sleep(150)
    const t = document.querySelector('main')?.innerText ?? document.body.innerText
    const len = t.length
    if (!seen.length || seen.at(-1).len !== len) seen.push({ at: i * 150, len, stop: /\bStop\b/.test(t), card: /Review & save/.test(t) })
  }
  report('samples (ms, page text length, Stop shown, card shown): ' + seen.map((s) => `${s.at}:${s.len}${s.stop ? 'S' : ''}${s.card ? 'C' : ''}`).join(' '))
  const convs = await invoke('list_conversations')
  report('newest conversation: ' + convs[0].title + ' · url ' + location.pathname)
  const msgs = await invoke('list_messages', { conversationId: convs[0].id })
  report('messages: ' + msgs.map((m) => `${m.role}${m.attachment ? '[' + m.attachment.name + ']' : ''}${m.draft ? '{' + m.draft.title + '}' : ''} "${m.text.slice(0, 50)}"`).join(' | '))
  report('DONE')
})().catch((e) => report('ERROR ' + (e?.message ?? JSON.stringify(e))))
