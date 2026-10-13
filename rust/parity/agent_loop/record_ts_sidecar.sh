#!/bin/bash
# record stdio of the TS sidecar for protocol study
mkdir -p "${RECORD_DIR:-/tmp/alp/rec}"
f=${RECORD_DIR:-/tmp/alp/rec}/$(date +%s%N)
exec > >(tee -a $f.out) 2> >(tee -a $f.err >&2)
tee -a $f.in | env -u PILOTDECK_AGENT_LOOP_SIDECAR_EXEC node ${PILOTDECK_ROOT:-/workspace/PilotDeck}/dist/src/cli/pilotdeck-agent-loop-sidecar.js "$@"
