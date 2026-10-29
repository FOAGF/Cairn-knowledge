// File names in external knowledge mode (FR-KN-EXT-15): a page's title is its file name, and Cairn
// follows Windows' rules on every system, so a folder synced between Linux and Windows works.
//
// MIRRORS RUST EXACTLY: `safe_file_name` in `crates/cairn-core/src/util.rs`, and `clean_name` /
// `clean_title` in `crates/cairn-core/src/knowledge/external/mod.rs`. Change them together. The UI
// only uses this to show "Saved as: …" while a title is typed; Rust does the saving.

const BAD = '<>:"/\\|?*'
/** Rust's `char::is_control` (general category Cc). */
const CONTROL = /\p{Cc}/u
/** Rust's `str::trim`: the Unicode White_Space property. */
const TRIM = /^\p{White_Space}+|\p{White_Space}+$/gu

/**
 * `<>:"/\|?*` and control characters become `-`; spaces are trimmed at both ends, dots and spaces at
 * the end; an empty result becomes `fallback`; Windows' reserved names (`CON`, `PRN`, `AUX`, `NUL`,
 * `COM1`–`COM9`, `LPT1`–`LPT9`, with or without an extension) get a `_`. Other scripts stay as they are.
 */
export function safeFileName(name: string, fallback: string): string {
  // Code points, as Rust's `chars()`.
  let out = [...name].map((c) => (CONTROL.test(c) || BAD.includes(c) ? '-' : c)).join('')
  out = out.replace(/^ +| +$/g, '')
  out = out.replace(/[. ]+$/, '')
  if (!out) out = fallback
  // The reserved names count with any extension (`con.txt`) and with trailing spaces before it.
  const stem = out
    .split('.')[0]
    .replace(/\p{White_Space}+$/u, '')
    .replace(/[a-z]/g, (c) => c.toUpperCase())
  const reserved =
    ['CON', 'PRN', 'AUX', 'NUL'].includes(stem) || ((stem.startsWith('COM') || stem.startsWith('LPT')) && stem.length === 4 && /[1-9]/.test(stem[3]))
  if (reserved) {
    // The stem is ASCII here, so string positions match Rust's byte positions.
    const dot = out.indexOf('.')
    const at = Math.min(stem.length, dot >= 0 ? dot : out.length)
    out = `${out.slice(0, at)}_${out.slice(at)}`
  }
  return out
}

/**
 * A name Cairn gives a file or folder in external mode: `safeFileName` of the trimmed name, then no
 * leading dots or spaces (hidden names are skipped by the scan), cleaned again while any are left.
 * Fallbacks: `page` for page titles, `Category` and `Group` for folders.
 */
export function cleanName(name: string, fallback: string): string {
  let out = safeFileName(name.replace(TRIM, ''), fallback)
  for (;;) {
    const stripped = out.replace(/^[. ]+/, '')
    if (stripped.length === out.length) return out
    out = safeFileName(stripped, fallback)
  }
}

/** The title a page gets in external mode for what was typed: its file name without `.md`. */
export function cleanTitle(title: string): string {
  return cleanName(title, 'page')
}
