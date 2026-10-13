#!/bin/bash
# Records the stdio of the TS agent-loop sidecar (one <ns>.in/.out/.err triple per process) for
# agent_loop_replay_parity.py / agent_loop_concurrent_parity.py. Regenerate the corpus with:
#   REC_DIR=/tmp/alp/rec PILOTDECK_AGENT_LOOP_SIDECAR_EXEC=$PWD/rust/parity/record_ts_sidecar.sh \
#     python3 tools/agent-loop-parity/run.py --pilotdeck-root $PWD --comparison same-version --surface gateway --scenario all --output /tmp/alp/recall
# (about 80 MB, not committed).
root="$(cd "$(dirname "$0")/../.." && pwd)"
dir="${REC_DIR:-/tmp/alp/rec}"; mkdir -p "$dir"
f="$dir/$(date +%s%N)"
exec > >(tee -a "$f.out") 2> >(tee -a "$f.err" >&2)
tee -a "$f.in" | env -u PILOTDECK_AGENT_LOOP_SIDECAR_EXEC node "$root/dist/src/cli/pilotdeck-agent-loop-sidecar.js" "$@"
