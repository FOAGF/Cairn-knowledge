// LaTeX formulas in Markdown (D-63). MarkdownView renders them with KaTeX (remark-math + rehype-katex,
// loaded on first use); this file only prepares the text so the syntax people expect works:
// - `$$` on lines of their own around a formula, or a line that is only `$$…$$`: a centred formula;
// - `\[…\]`: a centred formula; `\(…\)` and `$$…$$` inside a line: an inline formula;
// - `$…$` inside a line, by Pandoc's rule, so amounts such as "$5 and $10" stay text: the opening `$`
//   is followed by a non-space, and the next `$` on the line closes it only if it follows a non-space
//   and is not followed by a digit. A `$` that doesn't pair is escaped, so it shows as written.
// Code (fenced blocks and inline code spans) is never changed. The spelling check masks the same
// spans (crates/cairn-core/src/lint.rs, `math_ranges`).

/** Ranges [from, to) of fenced code blocks and inline code spans. */
function codeRanges(md: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = []
  let fence: { ch: string; len: number; from: number } | null = null
  for (let s = 0; s <= md.length; ) {
    const nl = md.indexOf('\n', s)
    const e = nl < 0 ? md.length : nl
    const m = /^ {0,3}(`{3,}|~{3,})/.exec(md.slice(s, e))
    if (!fence) {
      if (m) fence = { ch: m[1][0], len: m[1].length, from: s }
    } else if (m && m[1][0] === fence.ch && m[1].length >= fence.len && md.slice(s, e).trim() === m[1]) {
      ranges.push([fence.from, e])
      fence = null
    }
    if (nl < 0) break
    s = nl + 1
  }
  if (fence) ranges.push([fence.from, md.length])
  const inFence = (k: number) => ranges.some(([a, b]) => k >= a && k < b)
  const spans: Array<[number, number]> = []
  for (let k = 0; k < md.length; ) {
    if (md[k] !== '`' || inFence(k)) {
      k++
      continue
    }
    let n = 0
    while (md[k + n] === '`') n++
    // The closing run has exactly the same length.
    let close = -1
    for (let p = k + n; p < md.length; p++) {
      if (md[p] !== '`') continue
      let r = 0
      while (md[p + r] === '`') r++
      if (r === n) {
        close = p
        break
      }
      p += r - 1
    }
    if (close < 0) {
      k += n
      continue
    }
    spans.push([k, close + n])
    k = close + n
  }
  return [...ranges, ...spans].sort((a, b) => a[0] - b[0])
}

/** Escapes each single `$` on a line that doesn't open or close a formula by the rule above. */
function escapeLoneDollars(line: string): string {
  const singles: number[] = []
  for (let p = 0; p < line.length; p++) {
    if (line[p] !== '$') continue
    if (line[p + 1] === '$') {
      while (line[p + 1] === '$') p++
      continue
    }
    if (line[p - 1] !== '\\') singles.push(p)
  }
  const escape = new Set<number>()
  for (let s = 0; s < singles.length; ) {
    const open = singles[s]
    const close = singles[s + 1]
    const valid =
      close !== undefined &&
      line[open + 1] !== undefined &&
      !/\s/.test(line[open + 1]) &&
      !/\s/.test(line[close - 1]) &&
      !/\d/.test(line[close + 1] ?? '')
    if (valid) s += 2
    else {
      escape.add(open)
      s += 1
    }
  }
  if (!escape.size) return line
  let out = ''
  for (let p = 0; p < line.length; p++) out += (escape.has(p) ? '\\' : '') + line[p]
  return out
}

/** Text outside code: LaTeX delimiters become remark-math's, lone dollars are escaped. */
function prepareText(text: string): string {
  const converted = text.replace(/\\\[([\s\S]+?)\\\]/g, (_m, body: string) => `\n$$\n${body.trim()}\n$$\n`)
  let inBlock = false
  // `\(…\)` becomes inline `$$…$$` last, so a line holding only one doesn't count as a centred formula.
  return converted
    .split('\n')
    .map((line) => {
      if (line.trim() === '$$') {
        inBlock = !inBlock
        return line
      }
      if (inBlock) return line
      const whole = /^(\s*)\$\$(.+?)\$\$\s*$/.exec(line)
      if (whole) return `${whole[1]}$$\n${whole[1]}${whole[2].trim()}\n${whole[1]}$$`
      return escapeLoneDollars(line)
    })
    .join('\n')
    .replace(/\\\(([\s\S]+?)\\\)/g, (_m, body: string) => `$$${body}$$`)
}

/** Markdown ready for remark-math (see the top of this file). Text without `$` or `\(`/`\[` is returned as is. */
export function prepareMath(md: string): string {
  if (!/\$|\\\(|\\\[/.test(md)) return md
  let out = ''
  let at = 0
  for (const [from, to] of codeRanges(md)) {
    if (from < at) continue
    out += prepareText(md.slice(at, from)) + md.slice(from, to)
    at = to
  }
  return out + prepareText(md.slice(at))
}

/** Whether a text may hold a formula, so the math renderer is worth loading. */
export function mayHaveMath(md: string): boolean {
  return /\$|\\\(|\\\[/.test(md)
}
