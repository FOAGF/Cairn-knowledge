// D-72 probe, scenario C (second start): the switch backups are past the warning size, so the
// start-up pop-up shows (FR-KN-EXT-9); *Open switch backups* goes to their list. Run on the data
// folder ext-switch.js left (it sets the warning size to 0.000001 GB):
//   CAIRN_DATA_DIR=/tmp/x SHOTS=/tmp/shots PROBE_AFTER=6 PROBE_EXIT=30 \
//     PROBE_DRIVER=scripts/probes/ext-switch-warning-driver.sh scripts/probes/ext-probe.sh scripts/probes/ext-switch-warning.js
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const text = (el) => (el ? el.textContent.replace(/\s+/g, ' ').trim() : '(none)')
const $ = (id) => document.querySelector(`[data-testid="${id}"]`)
;(async () => {
  try {
    report('W1 at start: url=' + location.pathname + ', pop-up=' + !!$('switch-backup-warning') + ' "' + text($('switch-backup-warning')).slice(0, 260) + '"')
    report('@@shot-popup')
    await sleep(2500)
    $('open-switch-backups')?.click()
    await sleep(1500)
    const list = $('switch-backups')
    const r = list?.getBoundingClientRect()
    report(`W2 after Open switch backups: url=${location.pathname}${location.hash}, pop-up=${!!$('switch-backup-warning')}, list top=${Math.round(r?.top)} (window ${innerHeight}), rows: ${[...document.querySelectorAll('[data-testid="switch-backup-row"]')].map(text).join(' | ')} | ${text($('switch-backups-total'))}`)
    report('@@shot-list')
    await sleep(2500)
    report('DONE')
  } catch (e) {
    report('ERROR ' + (e?.stack || e?.message || JSON.stringify(e)))
  }
})()
