// Assistant regressions: reopening a proposal preserves unsaved edits; each picked page keeps
// its own identity, even with equal titles and edits around the references. Use a fresh data folder:
// CAIRN_DATA_DIR=/tmp/assistant-probe VITE_CAIRN_NO_SEED=1 PROBE_EXIT=90 scripts/ui-probe.sh scripts/probes/assistant-edit-references.js
// No real model is needed: the local provider deliberately refuses connections. We check the
// stored user message, not an AI reply. Only this throw-away data folder is changed.
;(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const text = (el) => el?.textContent.trim() ?? ''
  const wait = async (read) => {
    for (let i = 0; i < 100; i++) {
      const value = await read()
      if (value) return value
      await sleep(100)
    }
    throw new Error('Timed out')
  }
  const check = (ok, name) => report(`${ok ? 'PASS' : 'FAIL'} ${name}`)
  const go = async (path) => {
    history.pushState({}, '', path)
    dispatchEvent(new PopStateEvent('popstate'))
    await sleep(500)
  }
  const button = (label) => [...document.querySelectorAll('button')].find((b) => text(b) === label)
  const editor = () => document.querySelector('.cm-content')?.cmTile?.root?.view
  const input = () => document.querySelector('textarea')
  const type = async (value, caret = value.length) => {
    const el = input()
    el.focus()
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(el, value)
    el.setSelectionRange(caret, caret)
    el.dispatchEvent(new Event('input', { bubbles: true }))
    await sleep(100)
  }
  try {
    await invoke('update_settings', { patch: { language: 'en' } })
    const a = await invoke('create_category', { input: { name: 'Reference A', key: 'RFA', description: '', color: 'blue' } })
    const b = await invoke('create_category', { input: { name: 'Reference B', key: 'RFB', description: '', color: 'green' } })
    const create = (categoryId) => invoke('create_page', { input: { categoryId, title: 'Runbook', body: 'Original body.' } })
    const page = await create(a.id)
    await create(b.id)
    const chat = await invoke('create_conversation', { input: { title: 'Proposal regression' } })
    const proposal = await invoke('add_message', { conversationId: chat.id, input: {
      role: 'assistant', text: '', proposal: { reply: 'Update the page.', terms: [], shownTerms: [], termsCategoryId: null,
        rows: [{ change: { op: 'update_page', pageId: page.id, title: page.title, baseUpdatedAt: page.updatedAt,
          change: 'Replace the body.', oldBody: page.body, newBody: 'Proposed body.' }, ticked: true, state: 'pending', result: null, resultId: null }] },
    } })
    await go('/assistant/' + chat.id)
    ;(await wait(() => button('Edit…'))).click()
    const view = await wait(editor)
    check(view.state.doc.toString().includes('Proposed body.'), 'first Edit opens the proposed text')
    view.dispatch({ changes: { from: view.state.doc.length, insert: '\n\nUSER UNSAVED' } })
    await sleep(100)
    // Recheck the row to cover continuing an edit that already existed before this proposal.
    await invoke('set_proposal_row', { messageId: proposal.id, row: 0, ticked: true })
    await go('/assistant/' + chat.id)
    ;(await wait(() => button('Edit…'))).click()
    const reopened = await wait(editor)
    check(reopened.state.doc.toString().includes('USER UNSAVED'), 'reopening a proposal preserves unsaved edits')
    await wait(async () => !(await invoke('list_messages', { conversationId: chat.id })).find((m) => m.id === proposal.id).proposal.rows[0].ticked)
    check(true, 'continuing the edit unchecks the proposal so Confirm cannot apply it again')
    button('Cancel').click()

    const provider = await invoke('create_provider', { input: { name: 'Unavailable local test', kind: 'local', endpoint: 'http://127.0.0.1:1/v1', auth: 'none' } })
    const model = await invoke('create_model', { input: { providerId: provider.id, name: 'test', format: 'chat_completions' } })
    const pages = await invoke('list_pages', { filter: { limit: 10 } })
    const ids = pages.filter((p) => p.title === 'Runbook').map((p) => p.id)
    const pick = async (index) => {
      await wait(() => document.querySelectorAll('[role="listbox"] [role="option"]').length >= ids.length)
      document.querySelectorAll('[role="listbox"] [role="option"]')[index].click()
      await sleep(100)
    }
    const compose = async () => {
      const conversation = await invoke('create_conversation', { input: { title: 'Reference regression', modelId: model.id } })
      await go('/assistant/' + conversation.id)
      await wait(input)
      await type('  Compare [[')
      await pick(0)
      await type(input().value + 'and [[')
      await pick(1)
      return conversation.id
    }
    const send = async (conversationId) => {
      input().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
      return wait(async () => (await invoke('list_messages', { conversationId })).find((m) => m.role === 'user'))
    }
    const both = await compose()
    const sent = await send(both)
    check(sent.text === `Compare [Runbook](page:${ids[0]}) and [Runbook](page:${ids[1]})`, 'same-title pages retain separate IDs after trimming')

    const edited = await compose()
    await type('Prefix ' + input().value, 7)
    const editedMessage = await send(edited)
    check(editedMessage.text === `Prefix   Compare [Runbook](page:${ids[0]}) and [Runbook](page:${ids[1]})`, 'typing before references moves both identities')

    const removed = await compose()
    const remaining = input().value.replace('  Compare [[Runbook]] and ', '')
    await type(remaining, 0)
    const remainingMessage = await send(removed)
    check(remainingMessage.text === `[Runbook](page:${ids[1]})`, 'deleting the first reference preserves the second identity')

    const bracketed = await compose()
    const value = input().value.replace('  Compare ', '')
    await type(value, 0)
    await type('[' + value, 1)
    const bracketedMessage = await send(bracketed)
    check(bracketedMessage.text === `[[Runbook](page:${ids[0]}) and [Runbook](page:${ids[1]})`, 'inserting a bracket before a reference preserves its identity')
    report('DONE')
  } catch (e) {
    report('FAIL ' + e + '\n' + e.stack + '\n' + location.pathname + '\n' + document.body.innerText.slice(-2000))
  }
})()
