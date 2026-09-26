#!/bin/sh
# Stop the server so cache writes flush before RUN exits.
set -eu

if [ -n "${ACTIONS_RESULTS_URL:-}" ] && [ -n "${ACTIONS_RUNTIME_TOKEN:-}" ]; then
    export SCCACHE_GHA_ENABLED=on
fi

sccache --start-server

status=0
"$@" || status=$?

sccache --show-stats
sccache --stop-server
exit "$status"
