import type { ReactNode } from 'react'

// The search model added after installing (FR-RAG-EM-3, D-82): Settings › Search, the first-start
// card's note, the one-time Home notice, the progress line on Home and in the top bar, and the
// toasts. `mb` is the model's size in MB, rounded.
export const searchModel = {
  /** Settings › Search, the *Model* row of a model added after installing. */
  added: 'EmbeddingGemma (added, runs locally)',
  addedHint: 'Added after installing. Search works offline and nothing is sent out for indexing.',
  notInstalled: 'Not installed',
  notInstalledHint: (mb: number) =>
    `This copy of Cairn came without the search model. Keyword search works without it; search by meaning needs it (about ${mb} MB).`,
  /** The link's text. */
  terms: 'Gemma Terms of Use',
  /** The tick before the first download; `link` is `terms` as a link. */
  accept: (link: ReactNode) => <>I accept the {link}</>,
  accepted: (link: ReactNode) => <>You accepted the {link}.</>,
  download: 'Download',
  downloadInBackground: 'Download in the background',
  fromFile: 'From file…',
  fromZip: 'A .zip file…',
  fromFolder: 'A folder…',
  fromFileHint: 'A .zip file or a folder with model_q4.onnx, model_q4.onnx_data and tokenizer.json. Works offline.',
  /** The file type's name in the "Open" dialog. */
  zipFiles: 'Zip files',
  downloading: 'Downloading…',
  checking: 'Checking…',
  /** Done and total, in MB. */
  progress: (done: number, total: number) => `${done} of ${total} MB`,
  failedTitle: 'The search model couldn’t be added',
  /** Clears a failed download or import. */
  dismiss: 'Dismiss',
  removeTitle: 'Remove the search model?',
  removeBody:
    'Search by meaning stops until you add the model again. The search index is kept, so adding the model back needs no rebuild.',
  removing: 'Removing…',
  removed: 'Search model removed',
  /** FR-DATA-8: the first-start card's note on a lite package. */
  firstStartTitle: 'Search by meaning needs a download',
  firstStartText: (mb: number) =>
    `This copy of Cairn doesn’t include the search model (about ${mb} MB). Keyword search works without it. Once started, the download carries on in the background.`,
  /** The one-time Home notice: a copy with data but no search model. */
  noticeTitle: 'Add search by meaning',
  noticeText: (mb: number) =>
    `This copy of Cairn doesn’t include the search model (about ${mb} MB), so search finds pages by their words only. Download it to search by meaning too.`,
  /** The notice won't show again. */
  noticeDismiss: 'Dismiss',
  /** The progress line on Home; `pct` is the percentage done. */
  lineDownloading: (pct: number) => `Downloading the search model… ${pct}%`,
  lineChecking: (pct: number) => `Checking the search model… ${pct}%`,
  lineFailed: 'The search model couldn’t be added. Open Settings › Search.',
  /** The same in the top bar, shorter. */
  compact: (pct: number) => `Search model ${pct}%`,
  compactFailed: 'Search model not added',
  /** Toasts when a download or import ends; `error` comes from Rust. */
  ready: 'Search by meaning is ready; indexing starts now.',
  downloadFailed: (error: string) => `The search model download failed: ${error}`,
  importFailed: (error: string) => `The search model couldn’t be added: ${error}`,
}
