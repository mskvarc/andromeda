#!/bin/sh
# Run the given cargo command with sccache as the compiler wrapper, then stop
# the sccache server so every pending cache write is flushed before the RUN
# step's container exits.
#
# The storage backend is chosen by what the build was given. When the GitHub
# Actions runtime secrets are mounted, sccache stores one entry per compiled
# crate in the GitHub Actions cache, so a dependency bump recompiles only the
# crates whose inputs changed. Otherwise (a local build) it falls back to
# SCCACHE_DIR, which the Dockerfile backs with a BuildKit cache mount.
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
