#!/bin/bash
# D-72 checks: the external-mode probes run through scripts/ui-probe.sh, which also takes what
# they need (CAIRN_BIN, CAIRN_DEV_KNOWLEDGE_MODE=none, PROBE_OUT, PROBE_DRIVER, PROBE_APPLOG).
# Kept under this name for the probes and drivers that refer to it.
exec "$(dirname "$0")/../ui-probe.sh" "$@"
