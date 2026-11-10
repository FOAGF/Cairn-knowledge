// The tech news on Home and the default models (FR-HOME-7, D-91, D-92) in the real app, with the mock LLM
// (`scripts/mock_llm.py`, port MOCK_LLM_PORT, default 5298):
//   python3 -I scripts/mock_llm.py 5298 &          # then
//   CAIRN_DATA_DIR=/tmp/x PROBE_EXIT=120 scripts/ui-probe.sh scripts/probes/home-news.js
// Reports `@@<name>` lines where a driver may take a screenshot (`shot` in probes/ext-driver-lib.sh), and
// `@@done` at the end. Changes only the throw-away data folder.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const invoke = window.__TAURI_INTERNALS__.invoke
  const port = window.MOCK_LLM_PORT ?? 5298
  const go = async (path, ms = 1500) => {
    history.pushState({}, '', path)
    dispatchEvent(new PopStateEvent('popstate'))
    await sleep(ms)
  }
  const card = (title) => [...document.querySelectorAll('section h2')].find((h) => h.textContent === title)?.closest('section')
  const newsDone = async (ms = 30_000) => {
    const end = Date.now() + ms
    await sleep(300)
    while (Date.now() < end) {
      const v = await invoke('news')
      if (!v.fetching) return v
      await sleep(300)
    }
    return null
  }
  const shot = async (name) => {
    report(`@@${name}`)
    await sleep(2500)
  }

  // 1. Two models at the mock: one ticked Web search, one not (the default, so the chat uses it).
  const provider = await invoke('create_provider', { input: { name: 'Mock LLM', kind: 'openai_compatible', endpoint: `http://127.0.0.1:${port}/v1`, auth: 'none' } })
  const model = (label, webSearch) =>
    invoke('create_model', { input: { providerId: provider.id, name: label.toLowerCase(), label, format: 'chat_completions', vision: false, webSearch } })
  const searcher = await model('Searcher', true)
  const plain = await model('Plain', false)
  let s = await invoke('update_settings', { patch: { defaultModelId: plain.id } })
  report(`defaults: techNews=${s.techNews} topics=${JSON.stringify(s.newsTopics)} webSearchModelId=${s.webSearchModelId}`)

  // 2. Off by default: no card on Home, and nothing fetched.
  await go('/', 2500)
  report(`home while off: card=${!!card('Tech news')} news=${JSON.stringify(await invoke('news'))}`)

  // 3. Models & defaults: one card with the four default models; the only ticked model is automatic.
  await go('/settings/models')
  const defaults = card('Default models')
  const rows = defaults ? [...defaults.querySelectorAll('p.text-sm.font-medium')].map((p) => p.textContent) : []
  report(`default models rows: ${JSON.stringify(rows)}`)
  report(`web search picker shows: ${JSON.stringify(defaults?.querySelectorAll('button[role="combobox"]')[2]?.textContent)}`)
  report(`other cards: ${JSON.stringify([...document.querySelectorAll('section h2')].map((h) => h.textContent))}`)
  await shot('models')
  await go('/settings/search')
  report(`search tab cards: ${JSON.stringify([...document.querySelectorAll('section h2')].map((h) => h.textContent))}`)

  // 4. Settings › General › Home: the switch turns it on; the topics show then.
  await go('/settings/general')
  const home = card('Home')
  report(`general home card: ${!!home} topics field before=${!!home?.querySelector('input[aria-label="Topics"]')}`)
  home?.querySelector('button[role="switch"]')?.click()
  await sleep(1000)
  const topics = card('Home')?.querySelector('input[aria-label="Topics"]')
  report(`after switch: techNews=${(await invoke('get_settings')).techNews} topics field=${topics?.value}`)
  await invoke('update_settings', { patch: { newsTopics: 'AI, Rust' } })
  await sleep(500)
  await shot('general')

  // 5. Home fetches once today: seven stories, five shown.
  await go('/', 500)
  let v = await newsDone()
  await sleep(800)
  let news = card('Tech news')
  report(`fetched: stories=${v?.news.stories.length} topics=${v?.news.topics} model=${v?.news.modelLabel} triedOn=${v?.news.triedOn} error=${v?.news.error}`)
  report(`card: rows=${news?.querySelectorAll('li').length} more=${JSON.stringify([...(news?.querySelectorAll('button') ?? [])].map((b) => b.textContent).filter((t) => /more/.test(t)))} description=${JSON.stringify(news?.querySelector('h2')?.nextElementSibling?.textContent)}`)
  report(`first story: ${JSON.stringify(news?.querySelector('li')?.textContent)}`)
  await shot('home')

  // 6. Another visit today fetches nothing; Refresh does.
  v = await invoke('fetch_news', { language: 'en', force: false })
  report(`second visit: fetching=${v.fetching}`)

  // 7. A model that made no search: the error shows and the stories stay.
  await invoke('update_settings', { patch: { newsTopics: 'ignore this' } })
  await invoke('fetch_news', { language: 'en', force: true })
  v = await newsDone()
  await sleep(800)
  news = card('Tech news')
  report(`no search: error=${JSON.stringify(v?.news.error)} stories kept=${v?.news.stories.length} card error=${JSON.stringify(news?.querySelector('.bg-tint-rose')?.textContent)}`)
  await shot('error')

  // 8. In Chinese: the headlines come back in Chinese.
  await invoke('update_settings', { patch: { newsTopics: 'AI, Rust', language: 'zh-CN' } })
  await sleep(800)
  await invoke('fetch_news', { language: 'zh-CN', force: true })
  v = await newsDone()
  await sleep(800)
  report(`zh: first=${v?.news.stories[0]?.title} error=${v?.news.error} card title=${!!card('科技新闻')}`)
  await shot('zh')
  await invoke('update_settings', { patch: { language: 'en' } })

  // 9. The composer's 🌐: usable with the Plain model through the web search model; greyed out without one.
  await go('/assistant', 2500)
  const globe = () => document.querySelector('button[aria-label="Web search for the next message"]')
  report(`composer with Plain: globe found=${!!globe()} disabled=${globe()?.disabled}`)
  await invoke('update_model', { id: searcher.id, patch: { webSearch: false } })
  await sleep(1500)
  report(`no model can search: globe disabled=${globe()?.disabled}`)
  await go('/', 1500)
  const link = card('Tech news')?.querySelector('a[href="/settings/models"]')
  const refresh = [...(card('Tech news')?.querySelectorAll('button') ?? [])].find((b) => b.textContent.includes('Refresh'))
  report(`home without a web model: ${JSON.stringify(link?.parentElement?.textContent)} refresh disabled=${refresh?.disabled}`)
  await shot('nomodel')

  // 10. Off again: the card goes.
  await invoke('update_settings', { patch: { techNews: false } })
  await sleep(1000)
  report(`off again: card=${!!card('Tech news')}`)
  report('@@done')
})().catch((e) => report(`probe error: ${e?.message ?? e}`))
