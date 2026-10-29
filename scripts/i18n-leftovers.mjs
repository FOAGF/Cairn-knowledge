#!/usr/bin/env node
// Lists interface text still written in the code instead of the string table (NFR-I18N-2, D-79).
//
//   node scripts/i18n-leftovers.mjs [--summary] [path-prefix ...]
//
// It reads every .ts/.tsx file under app/src (or under the given prefixes, e.g. src/features/tasks)
// and prints JSX text and prose-like strings, one `file:line: text` per finding. It exits with 1
// when it finds any. It is a heuristic: a string that is not interface text (data Cairn stores,
// a log line, a value compared with) is marked with `// i18n-ignore` on its line or the line
// above, or a whole file with `// i18n-ignore-file` near the top.
import { createRequire } from 'node:module'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(fileURLToPath(import.meta.url), '../..')
const app = join(root, 'app')
const ts = createRequire(join(app, 'package.json'))('typescript')

const SKIP_FILES = [/^src\/i18n\//, /^src\/api\/bindings\.ts$/, /^src\/lib\/mock-data\.ts$/, /^src\/lib\/glossary-sample\.ts$/, /\.test\.tsx?$/]
/** Names, not words: the same in every language. */
const ALLOW = new Set(['Cairn', 'Markdown', 'LaTeX', 'Azure', 'Claude', 'DeepSeek', 'OpenAI', 'Ollama', 'PDF', 'DOCX', 'Ctrl+K', 'Ctrl', 'Shift', 'Tab', 'Esc', 'Enter'])
/** JSX attributes that never hold interface text. */
const SKIP_ATTRS = /^(className|class|key|id|type|name|href|src|to|role|variant|size|side|align|mode|value|defaultValue|htmlFor|autoComplete|inputMode|target|rel|lang|accept|method|d|viewBox|fill|stroke|style|orientation|sideOffset|alignOffset|as|tabIndex|data-[\w-]+|aria-(controls|describedby|labelledby|hidden|live|current|haspopup|expanded|selected|checked|pressed|orientation))$/
/** Calls whose string arguments are not interface text. */
const SKIP_CALLS = /^(cn|clsx|cva|twMerge|format|parseISO|formatDate|require|import|matchMedia|querySelector|querySelectorAll|getElementById|addEventListener|removeEventListener|getItem|setItem|removeItem|startsWith|endsWith|includes|indexOf|split|replace|replaceAll|match|matchAll|test|search|padStart|padEnd|join|RegExp|URL|URLSearchParams|createElement|setAttribute|getAttribute|removeAttribute|toggle|contains|add|remove|closest|invoke|listen|emit|useQuery|setQueryData|getQueryData|invalidateQueries|cancelQueries|setQueriesData|log|warn|error|debug|info|trace|toLocaleString|toLocaleDateString|toLocaleTimeString|DateTimeFormat|NumberFormat|has|get|set|delete|localeCompare|fetch|open|openUrl|revealItemInDir|setData|getData|scrollIntoView|animate|execCommand|computeStyle|getPropertyValue|setProperty)$/

const summary = process.argv.includes('--summary')
const prefixes = process.argv.slice(2).filter((a) => !a.startsWith('--')).map((p) => p.replace(/^app\//, '').replace(/\/$/, ''))

function files(dir) {
  const out = []
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) out.push(...files(p))
    else if (/\.tsx?$/.test(name) && !name.endsWith('.d.ts')) out.push(p)
  }
  return out
}

/** Looks like words a person reads, not a key, class list or path. */
function prose(text) {
  const t = text.trim()
  if (!/[A-Za-z]{2,}/.test(t) || ALLOW.has(t)) return false
  if (/^[\w./@:#%?=&+-]+$/.test(t) && !/^[A-Z][a-z]+$/.test(t)) return false
  // A class list: lower-case tokens, at least half of them with `-`, `:`, `/` or `[`.
  const tokens = t.split(/\s+/)
  if (tokens.every((w) => /^[a-z0-9:!\[\]\/._%#()&>*=+-]+$/.test(w)) && tokens.filter((w) => /[-:/[]/.test(w)).length * 2 >= tokens.length) return false
  return /[A-Za-z][,;]?\s+[A-Za-z(“"']/.test(t) || /^[A-Z][a-z]/.test(t) || /[.?!…:]$/.test(t)
}

function calleeName(call) {
  const e = call.expression
  if (ts.isIdentifier(e)) return e.text
  if (ts.isPropertyAccessExpression(e)) return e.name.text
  return ''
}

/** Whether a string at `node` is in a place that never holds interface text. */
function skipped(node) {
  for (let n = node, child = node; n; child = n, n = n.parent) {
    if (ts.isExpressionStatement(n) && n.expression === child) return true // 'use client'
    if (ts.isImportDeclaration(n) || ts.isExportDeclaration(n) || ts.isLiteralTypeNode(n) || ts.isCaseClause(n)) return true
    if (ts.isJsxAttribute(n)) return SKIP_ATTRS.test(n.name.getText())
    if (ts.isPropertyAssignment(n) && n.name === child) return true
    if (ts.isPropertyAssignment(n) && /^(queryKey|mutationKey|className|key|id|kind|type|variant|icon|href|to|path|route|pattern)$/.test(n.name.getText?.() ?? '')) return true
    if (ts.isElementAccessExpression(n) && n.argumentExpression === child) return true
    if (ts.isBinaryExpression(n) && /^(===|!==|==|!=|in)$/.test(n.operatorToken.getText())) return true
    if ((ts.isCallExpression(n) || ts.isNewExpression(n)) && n !== node) {
      const name = ts.isCallExpression(n) ? calleeName(n) : n.expression.getText()
      if (SKIP_CALLS.test(name) || name === 'Error') return true
      if (n.arguments?.includes(child)) return false
    }
    if (ts.isJsxExpression(n) || ts.isReturnStatement(n) || ts.isArrowFunction(n)) return false
  }
  return false
}

const findings = []
for (const file of files(join(app, 'src'))) {
  const rel = relative(app, file)
  if (SKIP_FILES.some((r) => r.test(rel))) continue
  if (prefixes.length && !prefixes.some((p) => rel === p || rel.startsWith(`${p}/`) || rel.startsWith(p))) continue
  const text = readFileSync(file, 'utf8')
  if (/^\s*\/\/ i18n-ignore-file/m.test(text.split('\n').slice(0, 10).join('\n'))) continue
  const lines = text.split('\n')
  const src = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
  const report = (node, value) => {
    const line = src.getLineAndCharacterOfPosition(node.getStart(src)).line
    if (/i18n-ignore/.test(lines[line]) || (line > 0 && /^\s*(\/\/|\{\/\*) i18n-ignore\b/.test(lines[line - 1]))) return
    findings.push({ rel, line: line + 1, value: value.replace(/\s+/g, ' ').trim() })
  }
  const visit = (node) => {
    if (ts.isJsxText(node)) {
      const v = node.getText(src)
      if (/[A-Za-z]{2,}/.test(v) && !ALLOW.has(v.trim())) report(node, v)
    } else if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      if (prose(node.text) && !skipped(node)) report(node, node.text)
    } else if (ts.isTemplateExpression(node)) {
      const parts = [node.head.text, ...node.templateSpans.map((s) => s.literal.text)].join(' ⟨⟩ ')
      if (prose(parts) && !skipped(node)) report(node, parts)
    }
    ts.forEachChild(node, visit)
  }
  visit(src)
}

if (summary) {
  const per = new Map()
  for (const f of findings) per.set(f.rel, (per.get(f.rel) ?? 0) + 1)
  for (const [rel, n] of [...per].sort((a, b) => b[1] - a[1])) console.log(`${String(n).padStart(4)}  ${rel}`)
} else {
  for (const f of findings) console.log(`${f.rel}:${f.line}: ${f.value}`)
}
console.log(`${findings.length} leftover string${findings.length === 1 ? '' : 's'}`)
process.exit(findings.length ? 1 : 0)
