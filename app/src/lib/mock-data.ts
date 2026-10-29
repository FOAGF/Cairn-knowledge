// Sample data for the UI prototype. Dates are relative to "now" so the
// prototype always looks current. Some items are in Chinese on purpose,
// to check fonts and layout (docs/requirements.md §8.6).

import type {
  ActivityEvent,
  KnowledgeGroup,
  KnowledgePage,
  PageHistoryVersion,
  Category,
  Task,
  TaskComment,
  TaskHistoryVersion,
  TaskLink,
  TaskPageLink,
} from './types'

const DAY = 24 * 60 * 60 * 1000

/** ISO timestamp `n` days ago (negative = in the future), at `hour`:00 local time. */
export function daysAgo(n: number, hour = 10): string {
  const d = new Date(Date.now() - n * DAY)
  d.setHours(hour, 0, 0, 0)
  // Never in the future (e.g. "today at 10:00" when it is 00:15).
  const latest = Date.now() - 30 * 60 * 1000
  return new Date(Math.min(d.getTime(), latest)).toISOString()
}

/** `YYYY-MM-DD` for `n` days from today (negative = past). */
export function dayFromToday(n: number): string {
  const d = new Date(Date.now() + n * DAY)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

// ------------------------------------------------------------------ Categories

export const categories: Category[] = [
  {
    id: 'p-general',
    name: 'General',
    key: 'GEN',
    description: 'Tasks and knowledge that are not specific to a category.',
    color: 'slate',
    status: 'active',
    isGeneral: true,
    createdAt: daysAgo(120),
  },
  {
    id: 'p-pipeline',
    name: 'Data Pipeline',
    key: 'DPL',
    description: 'Ingestion and processing of field sensor data.',
    color: 'blue',
    status: 'active',
    createdAt: daysAgo(90),
  },
  {
    id: 'p-portal',
    name: 'Customer Portal',
    key: 'PRT',
    description: 'Web portal for customer reports and dashboards.',
    color: 'purple',
    status: 'active',
    createdAt: daysAgo(80),
  },
  {
    id: 'p-sensor',
    name: 'Sensor Platform',
    key: 'SNS',
    description: 'Firmware and calibration for the field sensor units.',
    color: 'green',
    status: 'active',
    createdAt: daysAgo(60),
  },
  {
    id: 'p-learning',
    name: 'Learning',
    key: 'LRN',
    description: 'Rust, LLM tooling and other things I am learning.',
    color: 'amber',
    status: 'active',
    createdAt: daysAgo(45),
  },
]

// --------------------------------------------------------------------- Tasks

type TaskSeed = Omit<Task, 'id' | 'updatedAt'> & { updatedAt?: string }

const taskSeeds: TaskSeed[] = [
  // Data Pipeline
  { key: 'DPL-1', type: 'general', categoryId: 'p-pipeline', title: 'Set up nightly ingestion job', description: 'Schedule the ingestion job to run at 02:00 and write results to the staging bucket.\n\nSee [[Ingestion job runbook]].', status: 'done', priority: 'high', labels: ['infra'], createdAt: daysAgo(40), completedAt: daysAgo(30) },
  { key: 'DPL-2', type: 'bug', categoryId: 'p-pipeline', title: 'Fix timezone bug in sensor timestamps', description: 'Timestamps from units in UTC+8 are shifted by one hour after DST change.\n\nRelated to #SNS-4.', status: 'done', priority: 'high', labels: [], createdAt: daysAgo(21), completedAt: daysAgo(9) },
  { key: 'DPL-3', type: 'feature', categoryId: 'p-pipeline', title: 'Migrate storage to partitioned tables', description: 'Partition by month to speed up dashboard queries.\n\n- [x] Draft schema\n- [ ] Backfill 2025 data\n- [ ] Switch readers', status: 'in_progress', priority: 'high', labels: ['database'], createdAt: daysAgo(12), dueDate: dayFromToday(3) },
  { key: 'DPL-4', type: 'general', categoryId: 'p-pipeline', title: 'Waiting for access to the production replica', description: 'Requested read access from the platform team. Blocks #DPL-3 backfill.', status: 'blocked', priority: 'medium', labels: ['access'], createdAt: daysAgo(8) },
  { key: 'DPL-5', type: 'feature', categoryId: 'p-pipeline', title: 'Add data quality checks', description: 'Null checks, range checks and duplicate detection for incoming readings.', status: 'todo', priority: 'medium', labels: ['quality'], createdAt: daysAgo(5), dueDate: dayFromToday(9) },
  { key: 'DPL-6', type: 'general', categoryId: 'p-pipeline', title: '整理部署文档', description: '把部署步骤整理成知识页面，方便以后查找。', status: 'todo', priority: 'low', labels: ['docs'], createdAt: daysAgo(3) },
  // Customer Portal
  { key: 'PRT-1', type: 'feature', categoryId: 'p-portal', title: 'Design monthly report page', description: 'Wireframes for the monthly summary report.', status: 'done', priority: 'medium', labels: ['design'], createdAt: daysAgo(35), completedAt: daysAgo(26) },
  { key: 'PRT-2', type: 'feature', categoryId: 'p-portal', title: 'Implement CSV export', description: 'Users need to export the report table as CSV.', status: 'in_progress', priority: 'medium', labels: [], createdAt: daysAgo(10), dueDate: dayFromToday(0) },
  { key: 'PRT-3', type: 'bug', categoryId: 'p-portal', title: 'Login page shows blank screen on Safari', description: 'Reported by two customers. Cannot reproduce on Chrome.', status: 'todo', priority: 'high', labels: [], createdAt: daysAgo(2), dueDate: dayFromToday(1) },
  { key: 'PRT-4', type: 'feature', categoryId: 'p-portal', title: 'Dark mode for dashboards', description: '', status: 'wont_do', priority: 'low', labels: [], createdAt: daysAgo(30), completedAt: daysAgo(20) },
  { key: 'PRT-5', type: 'general', categoryId: 'p-portal', title: 'Accessibility review', description: 'Check contrast and keyboard navigation before release.', status: 'todo', priority: 'low', labels: ['quality'], createdAt: daysAgo(4), dueDate: dayFromToday(14) },
  // Sensor Platform
  { key: 'SNS-1', type: 'general', categoryId: 'p-sensor', title: 'Calibrate pressure sensors batch 7', description: 'Follow [[Pressure sensor calibration]].', status: 'done', priority: 'high', labels: ['calibration'], createdAt: daysAgo(18), completedAt: daysAgo(11) },
  { key: 'SNS-2', type: 'general', categoryId: 'p-sensor', title: 'Firmware 2.4 release notes', description: 'Collect changes since 2.3 and publish internally.', status: 'in_progress', priority: 'medium', labels: ['firmware', 'docs'], createdAt: daysAgo(6), dueDate: dayFromToday(2) },
  { key: 'SNS-3', type: 'bug', categoryId: 'p-sensor', title: 'Investigate battery drain on unit #42', description: 'Battery drops 20% per day since the last update.', status: 'blocked', priority: 'high', labels: ['firmware'], createdAt: daysAgo(7) },
  { key: 'SNS-4', type: 'feature', categoryId: 'p-sensor', title: 'Switch units to UTC timestamps', description: 'Long-term fix for #DPL-2.', status: 'todo', priority: 'medium', labels: ['firmware'], createdAt: daysAgo(9), dueDate: dayFromToday(6) },
  // Learning
  { key: 'LRN-1', type: 'general', categoryId: 'p-learning', title: 'Finish Rust ownership chapter', description: 'Chapters 4–5 of the Rust book. Notes in [[Rust ownership basics]].', status: 'done', priority: 'medium', labels: ['rust'], createdAt: daysAgo(20), completedAt: daysAgo(14) },
  { key: 'LRN-2', type: 'general', categoryId: 'p-learning', title: 'Build a small Tauri hello-world', description: '', status: 'in_progress', priority: 'medium', labels: ['rust', 'tauri'], createdAt: daysAgo(5), dueDate: dayFromToday(4) },
  { key: 'LRN-3', type: 'general', categoryId: 'p-learning', title: '阅读 RAG 检索论文', description: '重点看混合检索（关键词 + 向量）的部分。', status: 'todo', priority: 'low', labels: ['llm'], createdAt: daysAgo(1), dueDate: dayFromToday(10) },
  // General
  { key: 'GEN-1', type: 'general', categoryId: 'p-general', title: 'Renew VPN certificate', description: 'Certificate expires end of month. Steps in [[VPN setup]].', status: 'todo', priority: 'high', labels: ['admin'], createdAt: daysAgo(3), dueDate: dayFromToday(5) },
  { key: 'GEN-2', type: 'general', categoryId: 'p-general', title: 'Submit expense claim for September', description: '', status: 'done', priority: 'low', labels: ['admin'], createdAt: daysAgo(15), completedAt: daysAgo(12) },
  { key: 'GEN-3', type: 'general', categoryId: 'p-general', title: 'Clean up old laptop backups', description: '', status: 'todo', priority: 'low', labels: [], createdAt: daysAgo(6) },
]

export const tasks: Task[] = taskSeeds.map((t, i) => ({
  ...t,
  id: `t-${t.key.toLowerCase()}`,
  updatedAt: t.updatedAt ?? t.completedAt ?? daysAgo(Math.max(0, 3 - (i % 4))),
}))

const tid = (key: string) => `t-${key.toLowerCase()}`

export const taskLinks: TaskLink[] = [
  { id: 'tl-1', type: 'relates', fromTaskId: tid('DPL-2'), toTaskId: tid('SNS-4') },
  { id: 'tl-2', type: 'parent', fromTaskId: tid('DPL-3'), toTaskId: tid('DPL-4') },
  { id: 'tl-3', type: 'parent', fromTaskId: tid('DPL-3'), toTaskId: tid('DPL-5') },
  { id: 'tl-4', type: 'relates', fromTaskId: tid('LRN-2'), toTaskId: tid('LRN-1') },
  { id: 'tl-5', type: 'parent', fromTaskId: tid('SNS-2'), toTaskId: tid('SNS-3') },
]

export const taskComments: TaskComment[] = [
  { id: 'c-1', taskId: tid('DPL-2'), text: 'Found it: the parser assumed local time. Fix in review.', createdAt: daysAgo(12, 14) },
  { id: 'c-2', taskId: tid('DPL-2'), text: 'Blocked until the firmware team confirms the unit clock behaviour.', createdAt: daysAgo(15, 9) },
  { id: 'c-3', taskId: tid('DPL-2'), text: 'Confirmed with firmware team. Deployed the fix, verified on staging. See [[Timezone handling]].', createdAt: daysAgo(9, 16) },
  { id: 'c-4', taskId: tid('DPL-3'), text: 'Schema draft done. Backfill waits for #DPL-4.', createdAt: daysAgo(4, 11) },
  { id: 'c-5', taskId: tid('SNS-3'), text: 'Need the unit shipped back for bench testing. 已联系现场同事寄回。', createdAt: daysAgo(2, 15) },
  { id: 'c-6', taskId: tid('SNS-1'), text: 'All 24 sensors within ±0.5% after calibration.', createdAt: daysAgo(11, 17) },
]

/** Daily history (one version per day). DPL-2 shows 3 blocked working days. */
export const taskHistory: TaskHistoryVersion[] = [
  { taskId: tid('DPL-2'), date: dayFromToday(-21), snapshot: { type: 'bug', title: 'Fix timezone bug in sensor timestamps', description: '', status: 'todo', priority: 'high', labels: [], categoryId: 'p-pipeline' } },
  { taskId: tid('DPL-2'), date: dayFromToday(-17), snapshot: { type: 'bug', title: 'Fix timezone bug in sensor timestamps', description: 'Timestamps from units in UTC+8 are shifted by one hour after DST change.', status: 'in_progress', priority: 'high', labels: [], categoryId: 'p-pipeline' } },
  { taskId: tid('DPL-2'), date: dayFromToday(-15), snapshot: { type: 'bug', title: 'Fix timezone bug in sensor timestamps', description: 'Timestamps from units in UTC+8 are shifted by one hour after DST change.', status: 'blocked', priority: 'high', labels: [], categoryId: 'p-pipeline' } },
  { taskId: tid('DPL-2'), date: dayFromToday(-12), snapshot: { type: 'bug', title: 'Fix timezone bug in sensor timestamps', description: 'Timestamps from units in UTC+8 are shifted by one hour after DST change.\n\nRelated to #SNS-4.', status: 'in_progress', priority: 'high', labels: [], categoryId: 'p-pipeline' } },
  { taskId: tid('DPL-2'), date: dayFromToday(-9), snapshot: { type: 'bug', title: 'Fix timezone bug in sensor timestamps', description: 'Timestamps from units in UTC+8 are shifted by one hour after DST change.\n\nRelated to #SNS-4.', status: 'done', priority: 'high', labels: [], categoryId: 'p-pipeline' } },
  { taskId: tid('SNS-1'), date: dayFromToday(-18), snapshot: { type: 'general', title: 'Calibrate pressure sensors batch 7', description: '', status: 'todo', priority: 'high', labels: ['calibration'], categoryId: 'p-sensor' } },
  { taskId: tid('SNS-1'), date: dayFromToday(-16), snapshot: { type: 'general', title: 'Calibrate pressure sensors batch 7', description: 'Follow [[Pressure sensor calibration]].', status: 'in_progress', priority: 'high', labels: ['calibration'], categoryId: 'p-sensor' } },
  { taskId: tid('SNS-1'), date: dayFromToday(-11), snapshot: { type: 'general', title: 'Calibrate pressure sensors batch 7', description: 'Follow [[Pressure sensor calibration]].', status: 'done', priority: 'high', labels: ['calibration'], categoryId: 'p-sensor' } },
  { taskId: tid('DPL-4'), date: dayFromToday(-8), snapshot: { type: 'general', title: 'Waiting for access to the production replica', description: 'Requested read access from the platform team.', status: 'blocked', priority: 'medium', labels: ['access'], categoryId: 'p-pipeline' } },
]

// ----------------------------------------------------------------- Knowledge

export const groups: KnowledgeGroup[] = [
  { id: 'g-gen-it', categoryId: 'p-general', name: 'IT & Access', description: 'VPN, accounts, certificates and other IT access how-tos.' },
  { id: 'g-gen-admin', categoryId: 'p-general', name: 'Admin', description: 'Expenses, leave and other office admin procedures.' },
  { id: 'g-dpl-ops', categoryId: 'p-pipeline', name: 'Operations', description: 'Runbooks for running and fixing the ingestion jobs.' },
  { id: 'g-dpl-design', categoryId: 'p-pipeline', name: 'Design decisions', description: 'Why the pipeline is built the way it is: storage, time zones, schemas.' },
  { id: 'g-prt-front', categoryId: 'p-portal', name: 'Front end', description: 'Browser quirks and front-end notes for the customer portal.' },
  { id: 'g-sns-cal', categoryId: 'p-sensor', name: 'Calibration', description: 'Calibration procedures and acceptance criteria for the sensors.' },
  { id: 'g-sns-fw', categoryId: 'p-sensor', name: 'Firmware', description: 'Firmware releases, debugging and power issues.' },
  { id: 'g-lrn-rust', categoryId: 'p-learning', name: 'Rust', description: 'Notes from learning Rust and Tauri.' },
  { id: 'g-lrn-llm', categoryId: 'p-learning', name: 'LLM & RAG', description: 'Retrieval, embeddings and other LLM tooling notes.' },
]

type PageSeed = Omit<KnowledgePage, 'createdAt' | 'updatedAt' | 'checkStatus' | 'origin'> &
  Partial<Pick<KnowledgePage, 'checkStatus' | 'origin'>> & { created: number; updated: number }

const pageSeeds: PageSeed[] = [
  {
    id: 'k-vpn', categoryId: 'p-general', groupId: 'g-gen-it', title: 'VPN setup', tags: ['vpn', 'network'], created: 60, updated: 2,
    sourceInfo: 'IT service desk article, 2026',
    body: `# VPN setup

1. Install the VPN client **v5.2** from the software centre.
2. Server address: \`vpn2.corp.example\`
3. Sign in with your company account; approve the MFA prompt.

> Certificates expire yearly. Renewal steps are below.

## Renewing the certificate
- Open the client → *Settings* → *Certificates* → **Renew**.
- Restart the client.

Related task: #GEN-1`,
  },
  {
    id: 'k-expense', categoryId: 'p-general', groupId: 'g-gen-admin', title: 'Expense claim process', tags: ['admin'], created: 50, updated: 12,
    body: `# Expense claim process

- Submit within **30 days** of purchase.
- Attach receipts as PDF.
- Claims over $500 need manager approval.`,
  },
  {
    id: 'k-ingest', categoryId: 'p-pipeline', groupId: 'g-dpl-ops', title: 'Ingestion job runbook', tags: ['runbook', 'infra'], created: 38, updated: 6,
    origin: 'task', originTaskId: 't-dpl-1',
    body: `# Ingestion job runbook

The nightly job runs at **02:00** and writes to the staging bucket.

## If the job fails
1. Check the job log for \`ConnectionReset\`.
2. Re-run with \`--resume\`.
3. If it fails twice, page the on-call engineer.

See also [[Timezone handling]].`,
  },
  {
    id: 'k-tz', categoryId: 'p-pipeline', groupId: 'g-dpl-design', title: 'Timezone handling', tags: ['timestamps', 'design'], created: 9, updated: 9,
    origin: 'task', originTaskId: 't-dpl-2',
    body: `# Timezone handling

**Decision:** store all timestamps in UTC; convert only for display.

- Sensor units currently send local time → converted on ingestion (fixed in #DPL-2).
- Long-term: units send UTC directly (#SNS-4).`,
  },
  {
    id: 'k-partition', categoryId: 'p-pipeline', groupId: 'g-dpl-design', title: 'Monthly table partitioning', tags: ['database', 'design'], created: 10, updated: 4,
    body: `# Monthly table partitioning

Partition readings by month. Queries for the dashboard only touch the last 3 partitions.

| Table | Partition key | Retention |
|---|---|---|
| readings | month(ts) | 5 years |
| events | month(ts) | 2 years |`,
  },
  {
    id: 'k-safari', categoryId: 'p-portal', groupId: 'g-prt-front', title: 'Safari quirks', tags: ['browser', 'frontend'], created: 25, updated: 20,
    body: `# Safari quirks

- \`Date\` parsing of \`YYYY-MM-DD HH:mm\` fails; use ISO with \`T\`.
- Third-party cookies are blocked by default.`,
  },
  {
    id: 'k-csv', categoryId: 'p-portal', title: 'CSV export format', tags: ['export'], created: 8, updated: 8, checkStatus: 'not_checked',
    body: `# CSV export format

UTF-8 with BOM so Excel opens Chinese text correctly. Comma separator, quoted strings.`,
  },
  {
    id: 'k-cal', categoryId: 'p-sensor', groupId: 'g-sns-cal', title: 'Pressure sensor calibration', tags: ['calibration', 'procedure'], created: 70, updated: 11,
    origin: 'file', sourceInfo: 'Vendor manual v3, section 4 (handwritten notes from the lab)',
    body: `# Pressure sensor calibration

1. Warm up the unit for **15 minutes**.
2. Apply reference pressure at 0, 50 and 100 kPa.
3. Record readings; accept if within **±0.5%**.
4. Write offsets to the unit with \`calib write\`.`,
  },
  {
    id: 'k-fw-release', categoryId: 'p-sensor', groupId: 'g-sns-fw', title: 'Firmware release checklist', tags: ['firmware', 'release'], created: 30, updated: 5,
    body: `# Firmware release checklist

- [ ] Version bumped
- [ ] Release notes written (#SNS-2)
- [ ] Tested on 3 units
- [ ] Signed build uploaded`,
  },
  {
    id: 'k-battery', categoryId: 'p-sensor', groupId: 'g-sns-fw', title: '电池耗电排查', tags: ['battery', 'firmware'], created: 2, updated: 2, checkStatus: 'not_checked',
    body: `# 电池耗电排查

1. 检查固件版本是否为 2.4-beta。
2. 用 \`power log\` 命令导出 24 小时功耗记录。
3. 关注无线模块的唤醒频率。

相关任务：#SNS-3`,
  },
  {
    id: 'k-rust-own', categoryId: 'p-learning', groupId: 'g-lrn-rust', title: 'Rust ownership basics', tags: ['rust'], created: 15, updated: 14,
    origin: 'task', originTaskId: 't-lrn-1',
    body: `# Rust ownership basics

- Each value has **one owner**.
- Borrowing: many \`&T\` **or** one \`&mut T\`.
- When the owner goes out of scope, the value is dropped.

\`\`\`rust
fn main() {
    let s = String::from("cairn");
    let len = length(&s); // borrow, no move
    println!("{s} has {len} chars");
}
fn length(s: &str) -> usize { s.len() }
\`\`\``,
  },
  {
    id: 'k-tauri', categoryId: 'p-learning', groupId: 'g-lrn-rust', title: 'Tauri commands', tags: ['rust', 'tauri'], created: 4, updated: 1,
    body: `# Tauri commands

Expose a Rust function to the front end with \`#[tauri::command]\` and call it with \`invoke()\`.`,
  },
  {
    id: 'k-hybrid', categoryId: 'p-learning', groupId: 'g-lrn-llm', title: 'Hybrid search', tags: ['rag', 'search'], created: 6, updated: 3,
    body: `# Hybrid search

Combine **keyword** (BM25 / FTS5) and **vector** results, then merge with reciprocal rank fusion.

混合检索在中文和英文混合的内容上效果更稳定。`,
  },
  {
    id: 'k-embed', categoryId: 'p-learning', title: 'Embedding model notes', tags: ['rag', 'embeddings'], created: 3, updated: 3, checkStatus: 'not_checked',
    body: `# Embedding model notes

EmbeddingGemma: 308M parameters, multilingual, ~2K token context. Quantised versions use much less RAM.`,
  },
  {
    // Two unrelated topics, for trying the topic check: the text of the M5 prompt check's `check-two`
    // sample (poc/h-create/samples/two-topics.md), which both of the owner's models flagged. From a file,
    // so the check always looks at topics (a manual page only above the word threshold).
    id: 'k-meeting', categoryId: 'p-general', title: 'Notes from the Tuesday meeting', tags: ['meeting'], created: 1, updated: 1, checkStatus: 'not_checked',
    origin: 'file', sourceInfo: 'Meeting notes, exported from the team chat',
    body: `# Notes from the Tuesday meeting

## Flow meter calibration
The EP-40 flow meters on MMU-03 and MMU-07 are due for calibration. Book the calibration tank at the
depot for Thursday. Last time MMU-03 was off by 3.5%, so check it first.

## Travel budget for site visits
Site visits next quarter are capped at four trips. Flights must be booked two weeks ahead, and hotel
receipts go to the admin team within five days of returning.`,
  },
]

export const pages: KnowledgePage[] = pageSeeds.map(({ created, updated, ...p }) => ({
  ...p,
  origin: p.origin ?? 'manual',
  checkStatus: p.checkStatus ?? 'checked',
  createdAt: daysAgo(created),
  updatedAt: daysAgo(updated, 15),
}))

export const pageHistory: PageHistoryVersion[] = [
  { pageId: 'k-vpn', date: dayFromToday(-60), snapshot: { title: 'VPN setup', body: '# VPN setup\n\n1. Install the VPN client **v5.0**.\n2. Server address: `vpn.corp.example`', tags: ['vpn'], groupId: 'g-gen-it' } },
  { pageId: 'k-vpn', date: dayFromToday(-20), snapshot: { title: 'VPN setup', body: '# VPN setup\n\n1. Install the VPN client **v5.2** from the software centre.\n2. Server address: `vpn2.corp.example`', tags: ['vpn', 'network'], groupId: 'g-gen-it' } },
  { pageId: 'k-vpn', date: dayFromToday(-2), snapshot: { title: 'VPN setup', body: pages[0].body, tags: ['vpn', 'network'], groupId: 'g-gen-it', sourceInfo: 'IT service desk article, 2026' } },
]

export const taskPageLinks: TaskPageLink[] = [
  { id: 'tp-1', taskId: tid('GEN-1'), pageId: 'k-vpn' },
  { id: 'tp-2', taskId: tid('DPL-1'), pageId: 'k-ingest' },
  { id: 'tp-3', taskId: tid('DPL-2'), pageId: 'k-tz' },
  { id: 'tp-4', taskId: tid('SNS-1'), pageId: 'k-cal' },
  { id: 'tp-5', taskId: tid('LRN-1'), pageId: 'k-rust-own' },
  { id: 'tp-6', taskId: tid('DPL-3'), pageId: 'k-partition' },
]

// ------------------------------------------------------------------ Activity

export const activity: ActivityEvent[] = [
  { id: 'a-1', kind: 'page_updated', refId: 'k-tauri', at: daysAgo(0, 9) },
  { id: 'a-2', kind: 'task_created', refId: 't-lrn-3', at: daysAgo(1, 16) },
  { id: 'a-3', kind: 'page_updated', refId: 'k-vpn', at: daysAgo(2, 15) },
  { id: 'a-4', kind: 'page_created', refId: 'k-battery', at: daysAgo(2, 11) },
  { id: 'a-5', kind: 'task_created', refId: 't-prt-3', at: daysAgo(2, 10) },
  { id: 'a-6', kind: 'task_completed', refId: 't-dpl-2', at: daysAgo(9, 16) },
]
