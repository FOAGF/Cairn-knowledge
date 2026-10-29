// Takes screenshots of app routes for visual review.
// Usage: node scripts/screenshot.mjs <outDir> <route> [route...]
//   Options via env: BASE_URL (default http://localhost:5173), THEME=dark, WIDTH, HEIGHT, FULL=1
import { chromium } from 'playwright'

const [outDir, ...routes] = process.argv.slice(2)
if (!outDir || routes.length === 0) {
  console.error('usage: node scripts/screenshot.mjs <outDir> <route> [route...]')
  process.exit(1)
}
const base = process.env.BASE_URL ?? 'http://localhost:5173'
const browser = await chromium.launch()
const page = await browser.newPage({
  viewport: { width: Number(process.env.WIDTH ?? 1440), height: Number(process.env.HEIGHT ?? 900) },
  colorScheme: process.env.THEME === 'dark' ? 'dark' : 'light',
})
const errors = []
page.on('pageerror', (e) => errors.push(e.message))
page.on('console', (m) => m.type() === 'error' && errors.push(m.text()))
if (process.env.THEME) {
  await page.goto(base)
  await page.evaluate((t) => {
    const raw = localStorage.getItem('cairn-prototype')
    const data = raw ? JSON.parse(raw) : { state: {}, version: 1 }
    data.state.settings = { ...(data.state.settings ?? {}), theme: t }
    localStorage.setItem('cairn-prototype', JSON.stringify(data))
  }, process.env.THEME)
}
for (const route of routes) {
  await page.goto(base + route, { waitUntil: 'networkidle' })
  await page.waitForTimeout(400)
  const name = route.replace(/[^a-z0-9]+/gi, '_').replace(/^_|_$/g, '') || 'home'
  const file = `${outDir}/${name}${process.env.THEME ? '-' + process.env.THEME : ''}.png`
  await page.screenshot({ path: file, fullPage: process.env.FULL === '1' })
  console.log('saved', file)
}
if (errors.length) console.log('PAGE ERRORS:\n' + [...new Set(errors)].join('\n'))
await browser.close()
