// The English interface text, the source the Chinese is typed against (NFR-I18N-2, D-79).
import { common } from './common'
import { labels } from './labels'
import { dates } from './dates'
import { layout } from './layout'
import { shared } from './shared'
import { editor } from './editor'
import { home } from './home'
import { tasks } from './tasks'
import { knowledge } from './knowledge'
import { creation } from './creation'
import { assistant } from './assistant'
import { categories } from './categories'
import { glossary } from './glossary'
import { settings } from './settings'
import { storage } from './storage'
import { searchModel } from './searchModel'

export const en = { common, labels, dates, layout, shared, editor, home, tasks, knowledge, creation, assistant, categories, glossary, settings, storage, searchModel }

export type Messages = typeof en
