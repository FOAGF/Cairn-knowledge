import type { ReactNode } from 'react'
import { plural } from '../util'

export const settings = {
  page: {
    title: 'Settings',
    description: 'Your profile, AI providers, search and backups',
  },
  /** The tabs, by their slug in the route. */
  tabs: {
    general: 'General',
    providers: 'AI providers',
    models: 'Models & defaults',
    knowledge: 'Knowledge',
    search: 'Search',
    data: 'Backup & data',
    about: 'About',
  },
  general: {
    title: 'General',
    description: 'Your name, how Cairn looks, and where your data and knowledge pages live',
    yourName: 'Your name',
    yourNameHint: 'Used in the greeting on the home page.',
    theme: 'Theme',
    themeHint: 'System follows your operating system.',
    themeSystem: 'System',
    themeLight: 'Light',
    themeDark: 'Dark',
    language: 'Language',
    languageHint: 'Follow system uses Chinese when your system language is Chinese, otherwise English.',
    languageSystem: 'Follow system',
    /** Each language's own name, the same in every language. */
    languageEnglish: 'English',
    languageChinese: '简体中文',
    dataFolder: 'Data folder',
    dataFolderHint: 'Where the database and attachments are stored. Everything stays on this computer.',
    dataFolderNextStart: (folder: string) =>
      `Where the database and attachments are stored. After the next restart: ${folder} (existing data is not moved).`,
    changeFolder: 'Change…',
    folderChangeInDev: 'Changing the data folder is not available in development builds.',
  },
  dataFolderDialog: {
    title: 'Change the data folder',
    description:
      'Enter the full path of a folder. The change applies after you restart Cairn. Your existing data is not moved: copy it to the new folder yourself, or Cairn starts there with empty data.',
    placeholder: '/home/you/cairn-data',
    saved: 'Data folder saved. It is used the next time Cairn starts.',
  },

  // ---------------------------------------------------------------- General › Knowledge pages (D-72)
  knowledgeStorage: {
    labelExternal: 'Knowledge pages: external mode',
    labelInternal: 'Knowledge pages: internal mode',
    hintExternal: 'Pages are Markdown files in this folder that other programs can open too. Cairn’s backups don’t include the folder.',
    hintInternal: 'Pages are kept in Cairn’s own database and included in its backups.',
    openFolder: 'Open folder',
    reading: 'Reading…',
    rescan: 'Rescan',
    changeFolder: 'Change folder…',
    switchToInternal: 'Switch to internal mode…',
    switchToExternal: 'Switch to external mode…',
    missingTitle: 'The folder isn’t there',
    missingBody:
      'Cairn can’t find this folder, so nothing new is read from it and pages can’t be saved. If it is on a drive or a network folder, connect it, then use Rescan.',
    /** After the watcher's error (from Rust). */
    watchError: (error: string) => `${error} Rescan reads them, and tries watching the folder again.`,
    readFailed: (error: string) => `The folder could not be read: ${error}`,
    readAgain: (added: number, changed: number, renamed: number, removed: number) =>
      `Read again: ${plural(added, '# new page', '# new pages')}, ${changed} changed, ${renamed} renamed, ${removed} removed.`,
    readAgainDone: 'Read again.',
    skippedTitle: (n: number) => `${plural(n, '# file', '# files')} left out of the index`,
    skippedHint: 'These files in the folder are not read as pages. Fix them with another tool; the next read picks them up.',
  },

  // ---------------------------------------------------------------- the switch dialogs (FR-KN-EXT-8)
  switchToFolder: {
    titleOther: 'Switch to another folder?',
    titleExternal: 'Switch to external mode?',
    descriptionOther: 'Pages will come from the Markdown files in the new folder instead.',
    descriptionExternal: 'Pages become the Markdown files in this folder, and other tools can work on them too.',
    now: (folder: ReactNode) => <>Now: {folder}</>,
    change: 'Change…',
    backupFirst: 'A backup of the knowledge pages (with their history, task links and images) is made first and kept in Switch backups.',
    currentLeft:
      'The current folder is left as it is. Cairn’s details for its pages (checks, history, task links) are kept in that backup and come back if you switch to it again.',
    packedAway: 'Your current pages are packed away in that backup. They are not copied into the folder.',
    onlyMarkdown: (other: boolean) =>
      `Only the Markdown files already in the ${other ? 'new ' : ''}folder are used. Each folder in it becomes a category, and the folders inside those groups.`,
    unchanged: 'Tasks, categories, the glossary and conversations stay as they are.',
    searchRebuilt: 'Search by meaning is rebuilt; keyword search works at once.',
    starting: 'Starting…',
    confirm: 'Back up and switch',
  },
  switchToDatabase: {
    title: 'Switch to internal mode?',
    description:
      'A backup of the folder’s pages as Cairn knows them (with their history, task links and checks) is made first and kept in Switch backups, replacing the folder’s older one. Cairn then stops reading the folder and leaves it as it is. Tasks, categories, the glossary and conversations stay as they are.',
    question: 'Which pages should Cairn start with?',
    noBackup: 'There is no switch backup from internal mode, so knowledge starts empty.',
    choice: (pages: number, date: string) => `${plural(pages, '# page', '# pages')} from ${date}`,
    keptThen: (size: string) => `Kept in internal mode then · ${size}`,
    startEmpty: 'Start with no pages',
    startEmptyHint: 'Knowledge starts empty; the switch backups stay where they are.',
    onlyPages: 'Only the pages of a backup are loaded, with their history and their links to tasks that still exist.',
    starting: 'Starting…',
    confirm: 'Back up and switch',
  },

  // ---------------------------------------------------------------- AI providers (UI-SET-2)
  /** The provider kinds (FR-LLM-PV-1..4). */
  providerKinds: {
    azure_foundry: 'Azure AI Foundry',
    anthropic: 'Claude API',
    openai_compatible: 'OpenAI-compatible',
    local: 'Local model',
    deepseek: 'DeepSeek',
  },
  authModes: { api_key: 'API key', azure_cli: 'Azure CLI (az login)', none: 'None' },
  /** The request formats (FR-LLM-PV-7): API names. */
  requestFormats: {
    chat_completions: 'Chat Completions',
    responses: 'Responses API',
    anthropic_messages: 'Anthropic Messages',
  },
  providers: {
    intro: 'Connect one or more AI providers. Chat and background jobs use their models.',
    add: 'Add provider',
    emptyTitle: 'No AI providers yet',
    emptyBody: 'Add one to chat with your tasks and knowledge, and to let Cairn draft pages.',
  },
  providerDialog: {
    titleEdit: 'Edit provider',
    titleAdd: 'Add provider',
    description: 'Several providers can be configured at the same time.',
    kind: 'Kind',
    namePlaceholder: 'e.g. Work account',
    endpoint: 'Endpoint',
    endpointOptional: 'Endpoint (optional)',
    /** Under the endpoint, per provider kind. */
    kindHints: {
      azure_foundry:
        'Ends in /openai/v1 for GPT and other models, or /anthropic for Claude. Older resources use https://<resource>.openai.azure.com with an API version.',
      anthropic: 'Leave empty to use the Claude API at its usual address.',
      openai_compatible: 'Any service that speaks the OpenAI format. The address usually ends in /v1.',
      local: 'Ollama or LM Studio on this computer. Leave empty for the Ollama address.',
      deepseek: "DeepSeek's own API. Leave empty to use its usual address.",
    },
    /** The Azure endpoint style read from the address. */
    style: {
      openai_v1: 'Detected: OpenAI-compatible endpoint (/openai/v1). No API version needed.',
      anthropic: 'Detected: Claude on Azure (/anthropic). Models use the Anthropic Messages format.',
      classic: 'Detected: older Azure endpoint. It needs an API version.',
    },
    apiVersion: 'API version',
    apiVersionHint: "For example 2025-04-01-preview. Leave empty to use Cairn's default.",
    authentication: 'Authentication',
    keyLater: 'You can paste the key after adding the provider.',
    add: 'Add provider',
  },
  /** A provider's sign-in (FR-LLM-PV-1a, NFR-SEC-1). */
  auth: {
    none: 'No authentication needed for this endpoint.',
    /** `detail` comes from Rust. */
    signedIn: (account: ReactNode, detail: string) => (
      <>
        Signed in{account ? <> as {account}</> : ''}. {detail}
      </>
    ),
    notSignedIn: (detail: string) => `Not signed in. ${detail}`,
    notChecked: (
      <>
        Uses your existing <code>az login</code> session. Not checked yet.
      </>
    ),
    checking: 'Checking…',
    check: 'Check sign-in',
    keyStored: 'Key stored in the system keychain',
    keyPlaceholder: 'Paste API key',
    keyAria: 'API key',
    keySavedAria: 'API key (saved)',
    replaceKey: 'Replace key',
    setKey: 'Set key',
    replace: 'Replace',
    storedNote: 'Stored in the system keychain, never exported.',
  },
  modelsTable: {
    title: 'Models',
    add: 'Add model',
    empty: 'No models yet. Add one to use this provider.',
    label: 'Label',
    deployment: 'Deployment',
    modelName: 'Model name',
    format: 'Request format',
    effort: 'Effort',
    vision: 'Vision',
    testAria: (model: string) => `Test ${model}`,
    testTitle: 'Test connection',
    editAria: (model: string) => `Edit ${model}`,
    deleteAria: (model: string) => `Delete ${model}`,
  },
  providerCard: {
    claudeDefault: 'Claude API (default address)',
    disabled: 'Disabled',
    enabled: 'Enabled',
    enableAria: (provider: string) => `Enable ${provider}`,
    editAria: 'Edit provider',
    deleteAria: 'Delete provider',
    offNote: 'Its models are hidden from the model pickers and not used for background jobs until you turn it back on. Settings and keys are kept.',
    authentication: (mode: string) => `Authentication · ${mode}`,
    deleteTitle: (provider: string) => `Delete ${provider}?`,
    deleteBody: (models: number) => `Its ${plural(models, '# model', '# models')} and stored key are removed too.`,
  },
  /** The effort part of the model dialog (FR-LLM-EF-3, -4). */
  effortEditor: {
    title: 'Effort',
    hint: 'Shows the effort picker in chat. Never sent to a model unless this is on.',
    levels: 'Levels',
    levelsHint: 'The choices in the chat picker. Use the words your model understands.',
    removeLevel: (level: string) => `Remove level ${level}`,
    addLevel: 'Add level',
    defaultLevel: 'Default level',
    defaultLevelHint: "Used when chat doesn't choose one. “Not set” sends nothing.",
    howSent: 'How it is sent',
    autoHint: "Auto: Cairn uses the provider's own setting for each level.",
    auto: 'Auto',
    custom: 'Custom: a value per level',
    valuePlaceholder: 'e.g. 4096',
    valueAria: (level: string) => `Value for ${level}`,
    customNote: 'A whole number is a Claude thinking budget in tokens. Anything else is sent as the reasoning effort.',
  },
  modelDialog: {
    titleAdd: 'Add model',
    titleEdit: 'Edit model',
    deploymentName: 'Deployment name',
    modelName: 'Model name',
    azureHint: "Azure can't list your deployments from here. Type the deployment name exactly as it appears in Azure AI Foundry.",
    namePlaceholder: 'e.g. gpt-5-mini',
    fetch: 'Fetch models',
    noneListed: 'The provider lists no models.',
    pickOne: (models: number) => `Pick one of ${models} models`,
    label: 'Label',
    labelHint: 'Shown in the model picker. Leave empty to use the name.',
    labelPlaceholder: 'Shown in the model picker',
    format: 'Request format',
    vision: 'Vision',
    visionHint: 'Can read images. Needed for the OCR model.',
    webSearch: 'Web search',
    /** FR-LLM-AS-12, D-69. */
    webSearchDeepSeek: (
      <>
        Lets you use 🌐 in the chat with this model. Cairn searches through DeepSeek's Search API and the model reads the results; DeepSeek charges for each search or page read (USD 0.75 per 1,000 from 1 November 2026), up to the limit per message in Models &amp; defaults. <em>Test connection</em> can check it.
      </>
    ),
    webSearchOther: (
      <>
        The provider offers built-in web search (Claude's web search, OpenAI's Responses API). Lets you use 🌐 in the chat with this model. It turns itself off if the provider refuses or ignores it; <em>Test connection</em> can check it.
      </>
    ),
    contextSize: 'Context size',
    contextHint: 'Leave empty to use the size Cairn knows for this model, or to let Cairn find out.',
    contextPlaceholder: 'e.g. 128000',
    contextAria: 'Context size in tokens',
    tokens: 'tokens',
    contextInvalid: 'Enter a whole number of tokens, or leave it empty.',
    saving: 'Saving…',
  },
  /** *Test connection* (UI-SET-2, FR-LLM-EF-4). */
  testDialog: {
    title: 'Test connection',
    testing: 'Testing… this can take a few seconds.',
    checks: { basic: 'Basic reply', stream: 'Streaming', image: 'Image input', web: 'Web search' },
    effort: (level: string) => `Effort: ${level}`,
    passed: 'Passed',
    searchesNotTicked: 'This model searches the web, but Web search is not ticked.',
    didNotSearch: 'This model did not search the web, but Web search is ticked.',
    tick: 'Tick Web search',
    untick: 'Untick Web search',
    runAgain: 'Run again',
  },

  // ---------------------------------------------------------------- Models & defaults (FR-LLM-AS-6, UI-SET-3)
  defaults: {
    providerOff: "This model's provider is disabled. Background jobs will fail until you pick another model or re-enable the provider.",
    choose: 'Choose a model',
    providerDisabled: 'Provider disabled',
    defaultTitle: 'Default model',
    defaultDescription: 'For background jobs. The chat model is picked in the conversation.',
    background: 'Default background model',
    backgroundHint: 'Used for drafting pages, topic checks, group suggestions and merge suggestions.',
    assistantTitle: 'Assistant',
    assistantDescription: 'How much the Assistant sends with a question',
    sourceBudget: 'Source budget',
    sourceBudgetHint: 'Tokens of your notes and tasks sent with a question. Less when the model takes less. Default 8,000.',
    sourceBudgetAria: 'Source budget in tokens',
    historyBudget: 'History budget',
    historyBudgetHint: 'Tokens of the earlier conversation sent with a question. A conversation can be widened from the chat. Default 30,000.',
    historyBudgetAria: 'History budget in tokens',
    tokens: 'tokens',
    webSearches: 'Web searches per message',
    webSearchesHint: 'The most searches a model may make for one 🌐 message (1 to 20). Default 5.',
    ocrTitle: 'OCR model',
    ocrDescription: 'Reads text from images and scanned files',
    ocr: 'Vision / OCR model',
    ocrHint: 'Only models marked as vision-capable are listed.',
  },

  // ---------------------------------------------------------------- Knowledge
  knowledge: {
    limitsTitle: 'Limits',
    limitsDescription: 'Gentle warnings, never hard blocks',
    wordWarning: 'Word-count warning',
    wordWarningHint: 'Soft warning when a page grows past this many words (FR-KN-ED-4). Chinese characters count as one word each.',
    words: 'words',
    largeFile: 'Large-file warning',
    largeFileHint: 'Warn before importing a file bigger than this (FR-KN-UP-2).',
    mb: 'MB',
    duplicatesTitle: 'Duplicates',
    duplicatesDescription: 'The duplicate check when a page is saved or checked (FR-KN-DU-1)',
    sameTopic: 'Likely the same topic',
    sameTopicHint:
      'Pages at or above this similarity (0 to 1) are marked “Likely the same topic”; the others are shown as “Related”. Similarity comes from the search model, so it only counts once pages are indexed.',
    scoreInvalid: 'Enter a number above 0 and below 1.',
    atOnce: 'Pages checked at once',
    atOnceHint:
      'In “Check all unchecked”, this many pages are checked at the same time (1 to 20). More is faster but sends more requests to your model at once. Default 5.',
    spellingTitle: 'Spelling & grammar',
    spellingDescription: 'Powered by Harper, offline. Nothing you write leaves this computer.',
    spellcheck: 'Spell check',
    spellcheckHint: 'Default for new pages and tasks (FR-KN-ED-6).',
    grammar: 'Grammar check',
    grammarHint: 'Underlines grammar issues while you type (FR-KN-ED-7).',
    dictionaryTitle: 'Personal dictionary',
    dictionaryDescription: 'Words that are never marked as mistakes',
    removeWord: (word: string) => `Remove ${word}`,
    noWords: 'No words yet.',
    addWord: 'Add a word',
  },

  // ---------------------------------------------------------------- Search (FR-RAG-IX-5, UI-SET-3)
  search: {
    worker: {
      stopped: 'Stopped (starts when needed)',
      starting: 'Starting…',
      ready: 'Running',
      failed: 'Failed',
    },
    rebuilt: 'Index rebuilt. Pieces that changed are indexed in the background.',
    modelTitle: 'Search model',
    modelDescription: 'Used for meaning-based search',
    model: 'Model',
    modelHint: 'Built into the app. Search works offline and nothing is sent out for indexing.',
    builtIn: 'EmbeddingGemma (built-in, runs locally)',
    workerLabel: 'Search model worker',
    workerHint: 'A separate process that loads the model only when needed.',
    idle: 'Idle timeout',
    idleHint:
      'Applies to both AI workers (the search model and AI calls): each is stopped after this much idle time to free memory, and starts again when needed.',
    minutes: 'minutes',
    assistantTitle: 'Assistant',
    assistantDescription: 'Which results the Assistant sends to the model',
    relevance: 'Minimum relevance',
    relevanceHint:
      'A result found only by meaning needs at least this similarity (0 to 1) to be sent as a source. Results below it are not sent. Default 0.40.',
    indexTitle: 'Search index',
    indexDescription: 'Derived from your pages and tasks. It updates automatically when you save.',
    /** `ready` is the large number. */
    searchable: (ready: ReactNode, items: number) => (
      <>
        {ready}
        <span className="text-muted-foreground"> of {items} pages and tasks searchable by meaning</span>
      </>
    ),
    pieces: (ready: number, total: number) => `${ready} of ${total} pieces`,
    indexing: ' · indexing…',
    /** `error` comes from Rust. */
    stopped: (error: string) => `Indexing stopped: ${error} It tries again in a while; keyword search keeps working.`,
    loading: 'Loading…',
    rebuilding: 'Rebuilding…',
    rebuild: 'Rebuild index',
    alsoAfterImport: 'Also run automatically after an import.',
    cloudTitle: 'Cloud embeddings',
    cloudDescription: 'Use an embedding model from your cloud provider instead',
    cloud: 'Use cloud embeddings',
    later: 'later',
    cloudHint: 'Planned for a later version. Changing the embedding model will need a full index rebuild.',
  },

  // ---------------------------------------------------------------- Backup & data (UI-SET-4)
  data: {
    title: 'Export & import',
    description: 'Move all your data to another computer',
    exportLabel: 'Export everything',
    exportHint: 'One .zip file with all your data. API keys are never included.',
    export: 'Export…',
    importLabel: 'Import from a file',
    importHint: 'Replaces all current data. A backup is made first.',
    chooseFile: 'Choose file…',
  },
  /** NFR-REL-2. */
  backup: {
    title: 'Backup',
    description: 'A full export, made once a day while Cairn is open',
    daily: 'Automatic daily backup',
    dailyHint: 'Made when Cairn starts and once for each day it stays open.',
    folder: 'Backup folder',
    folderElsewhere: 'A copy in another folder, for example OneDrive, also protects you if this disk is lost.',
    folderDefault:
      'Inside the data folder: this protects against mistakes, not against losing the disk. Choose a OneDrive folder, for example, to be safe.',
    chooseFolder: 'Choose folder…',
    openFolder: 'Open folder',
    useDefault: 'Use default folder',
    keep: 'Keep the newest',
    keepHint: 'Only the newest N daily backups are kept; older ones are deleted. Backups made with ‘Back up now’ are kept until you delete them.',
    keepAria: 'Daily backups to keep',
    keepUnit: 'daily backups',
    last: 'Last backup',
    backingUp: 'Backing up…',
    backUpNow: 'Back up now',
    checkFailedTitle: 'The database check found a problem',
    checkFailedBody:
      'Backups are still made, but no old backup is deleted until a check passes. Your older backups may be the healthy ones, so keep them.',
    listTitle: 'Backups in this folder',
    listHint:
      'Cairn deletes only this computer’s own old daily backups. Other computers’ files, and earlier backups without a computer name, are never deleted.',
    empty: 'No backups yet.',
    listAria: 'Backups',
    kindDaily: 'Daily',
    kindManual: 'Manual',
    noComputer: 'Made before backup names carried the computer name',
    earlier: 'earlier backup',
    thisComputer: '(this computer)',
    restore: 'Restore…',
    /** `at` is a date and time; `error` comes from Rust. */
    lastFailed: (at: string | null, error: string) =>
      `The last backup failed${at ? ` (${at})` : ''}: ${error} Cairn tries again in an hour.`,
    noneYet: 'No backup from this computer yet.',
    /** The toast after *Back up now*. */
    saved: (file: string) => `Backup saved: ${file}`,
  },
  /** FR-DATA-1..5. */
  exportDialog: {
    title: 'Export everything',
    description: (
      <>
        Creates one <code>.zip</code> file you can restore on another computer.
      </>
    ),
    finished: (size: string) => `Export finished (${size})`,
    /** After the tables' row counts. */
    attachments: (n: number) => ` · ${n} attachments`,
    included: 'Included',
    includesTasks: 'Categories, tasks with comments and history',
    includesPages: 'Knowledge pages with history and pasted images, groups and links',
    includesConversations: 'Assistant conversations',
    includesSettings: 'App settings, providers and model names (no secrets)',
    externalNote:
      'Pages are files in your knowledge folder: the export holds Cairn’s index of them and the folder’s place, not the folder itself.',
    noKeysTitle: 'API keys are not exported',
    noKeysBody: 'You will enter them again on the new device. The search index is rebuilt after import.',
    exporting: 'Exporting…',
    export: 'Export…',
  },
  /** FR-DATA-3, -4, -6, FR-KN-EXT-14. */
  importDialog: {
    pickRoot: 'Where is this backup’s knowledge folder now?',
    titleFinished: 'Import finished',
    titleRefused: 'This file can’t be imported',
    titleAsking: 'Where is this backup’s folder?',
    titleWarning: 'Import only the tasks and categories?',
    titleConfirm: 'Replace all current data?',
    savedTo: (path: ReactNode) => <>Your previous data was saved to {path}</>,
    nothingChanged: 'Nothing was changed',
    askingBody: (folder: ReactNode) => (
      <>This backup keeps its knowledge pages as Markdown files in a folder, {folder}, which isn’t on this computer.</>
    ),
    askingHint: 'Choose where that folder is now, for example after copying it here. Its pages are read from there after the import.',
    warningTitle: 'Without its folder, most of this backup is left out',
    warningBody:
      'Only the tasks (with their comments, history, links, labels and images) and the categories are imported. Pages, the glossary, conversations and settings from this backup are not. All current data is replaced, after a backup of it, and Cairn then keeps knowledge in its own database, with no pages.',
    replaceTitle: 'This replaces everything',
    replaceBody:
      'All current categories, tasks, pages and conversations will be replaced by the contents of this file. A backup of your current data is made automatically first. Datasets cannot be merged.',
    readFrom: (folder: ReactNode) => <>Pages are read from the folder {folder}</>,
    change: 'Change…',
    withoutFolder: 'Import without the folder…',
    chooseFolder: 'Choose the folder…',
    importing: 'Importing…',
    importTasks: 'Back up and import tasks',
    replace: 'Back up and replace',
  },
  /** FR-KN-EXT-9. */
  switchBackups: {
    title: 'Switch backups',
    description: 'The knowledge pages, backed up before every switch of where they live (Settings › General)',
    openFolder: 'Open folder',
    about:
      'Each holds the pages with their history, task links and images; tasks and settings are in the daily backups. For each folder only the newest is kept; the ones from internal mode stay until you delete them.',
    empty: 'No switch backups yet.',
    listAria: 'Switch backups',
    /** The badge. */
    mode: { internal: 'Internal', external: 'External' },
    /** In the delete dialog's sentence. */
    modeText: { internal: 'internal mode', external: 'external mode' },
    pages: (n: number) => plural(n, '# page', '# pages'),
    total: (size: string, overWarning: boolean) =>
      `Together: ${size}${overWarning ? ' — more than the warning size, so every start suggests a clean-up.' : ''}`,
    warnLabel: 'Warn when switch backups pass',
    warnHint: 'Every start shows a reminder while all switch backups together are larger than this. Default 2 GB.',
    warnAria: 'Switch backup warning size in GB',
    gb: 'GB',
    warnInvalid: 'Enter a size above 0.',
    deleteTitle: 'Delete this switch backup?',
    deleteDetails: (date: string, pages: number, mode: string, size: string) =>
      `From ${date}: ${plural(pages, '# page', '# pages')}, ${mode}, ${size}. `,
    deleteForGood: 'It is deleted for good and can’t be used to switch back.',
    deleteConfirm: 'Delete backup',
  },
  /** NFR-SEC-3. */
  logs: {
    title: 'Logs',
    description: 'For troubleshooting. They never hold your pages, tasks, chats or keys.',
    size: 'Space the logs take',
    sizeHint: 'Logs are kept for 7 days and at most 50 MB; the oldest go first.',
    detailed: 'Detailed logs',
    detailedHint: 'Adds debug output until Cairn is next started. It is never left on by accident.',
    detailedAria: 'Detailed logs until the next start',
    open: 'Open logs folder',
    openHint: 'Opens the folder in the file manager.',
  },
  /** Development builds only. */
  demo: {
    title: 'Demo data',
    description: 'Development builds start with sample data',
    reset: 'Reset demo data',
    resetHint: 'Throws away your edits and restores the original samples.',
    resetButton: 'Reset…',
    confirmTitle: 'Reset demo data?',
    confirmBody: 'All your changes to categories, tasks, pages, conversations and settings will be lost.',
    resetting: 'Resetting…',
    confirm: 'Reset',
  },

  // ---------------------------------------------------------------- About (UI-SET-5)
  about: {
    version: 'Version',
    /** After the version number. */
    devBuild: ' (development build)',
    built: 'Built',
    commit: 'Commit',
    /** After the commit. */
    localChanges: ' + local changes',
    schema: 'Database schema',
    schemaVersion: (n: number) => `version ${n}`,
    unknown: 'Unknown',
  },
}
