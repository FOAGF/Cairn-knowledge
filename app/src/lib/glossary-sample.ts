// Development only: the sample glossary, sent to `devSeedGlossary` with fixed ids (like the other dev seeds).
// Category ids are the dev seed ids from mock-data.ts (p-sensor = Sensor Platform, ...), and the terms
// below really occur in the dev pages and tasks. Runs after the categories and pages are seeded.
import type { GlossarySeed } from '@/api/bindings'

const g = (id: string, term: string, otherNames: string[], description: string, extra: Partial<GlossarySeed> = {}): GlossarySeed => ({
  id: `gl-${id}`,
  term,
  otherNames,
  description,
  categoryId: null,
  ...extra,
})

export const glossarySeed = (): GlossarySeed[] => [
  // The owner's real example (global): auger is related to MMU because its description names it.
  g('mmu', 'MMU', ['Mobile Manufacturing Unit'], 'Truck that mixes bulk explosive on site and pumps it into the blast holes.'),
  g('auger', 'auger', [], 'The arm on an MMU that delivers the explosive into the blast hole.'),
  // Category-specific override: same term, different meaning inside Sensor Platform (FR-KN-GL-2).
  g('mmu-sns', 'MMU', ['Memory Management Unit'], 'Hardware that maps virtual addresses to physical memory on the controller board.', { categoryId: 'p-sensor' }),
  // Terms found in the dev pages and tasks.
  g('vpn', 'VPN', ['Virtual Private Network'], 'Encrypted connection to the company network. Needed for the production replica and internal tools. Sign-in needs MFA.'),
  g('mfa', 'MFA', ['Multi-factor authentication', '2FA'], 'Second proof of identity (phone prompt or code) on top of the password.'),
  g('utc', 'UTC', ['Coordinated Universal Time'], 'The time standard we store every timestamp in. Convert to local time only for display. DST does not change it.'),
  g('dst', 'DST', ['Daylight saving time'], 'Clock change in spring and autumn. A common cause of one-hour shifts when timestamps are not stored in UTC.'),
  g('csv', 'CSV', ['Comma-separated values'], 'Plain-text table format that Excel can open. Our exports use UTF-8 with BOM.', { categoryId: 'p-portal' }),
  g('kpa', 'kPa', ['kilopascal'], 'Unit of pressure. The calibration reference points are 0, 50 and 100 kPa.', { categoryId: 'p-sensor' }),
  g('firmware', '固件', ['firmware'], 'The software stored on the sensor unit itself. Updated over the air (OTA) or with a cable.', { categoryId: 'p-sensor' }),
  g('staging', 'staging bucket', [], 'Storage area where the nightly ingestion job first writes its results, before they are loaded into the tables.', { categoryId: 'p-pipeline' }),
  g('rag', 'RAG', ['Retrieval-augmented generation'], 'Giving an LLM the notes found by search so it can answer from them. Uses hybrid search (keyword plus vector), merged with RRF.', { categoryId: 'p-learning' }),
  g('bm25', 'BM25', [], 'Classic keyword ranking formula; the keyword half of hybrid search.', { categoryId: 'p-learning' }),
  g('rrf', 'RRF', ['Reciprocal rank fusion'], 'Way to merge two ranked result lists by adding 1/(k + rank) for every list an item appears in.', { categoryId: 'p-learning' }),
  g('fts5', 'FTS5', ['SQLite full-text search'], 'The SQLite module that provides keyword search with BM25 ranking.', { categoryId: 'p-learning' }),
]
