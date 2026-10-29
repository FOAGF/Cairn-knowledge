// Runs the checks in Rust and lists them with what the page itself can see.
(async () => {
  const out = document.getElementById('out')
  const lines = []
  lines.push('INFO webview ' + navigator.userAgent)
  const yahei = document.fonts && document.fonts.check('16px "Microsoft YaHei"', '中文')
  lines.push((yahei ? 'PASS' : 'FAIL') + ' font Microsoft YaHei available to the page')
  try {
    const res = await window.__TAURI_INTERNALS__.invoke('checks')
    lines.push(...res)
  } catch (e) {
    lines.push('FAIL Rust command: ' + JSON.stringify(e))
  }
  out.textContent = lines.join('\n')
})()
