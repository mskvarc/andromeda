#!/usr/bin/env bash
# Runs one tree of andromeda-test-suite against the stack it needs, and copies the tree's JUnit report to REPORT_DIR
# (default target/suite-reports). The broker, database and runner images are rebuilt from the working tree, except each
# one ANDROMEDA_IMAGE, ANDROMEDA_POSTGRES_IMAGE or ANDROMEDA_TEST_SUITE_RUNNER_IMAGE names, which is pulled instead. The stack is taken down
# afterwards with its databases, keeping the shared test-suite-target and test-suite-cargo-registry volumes. Two runs on
# one Docker host cannot overlap: every stack has a fixed project name and shares those volumes. A runner image that
# carries the suite's nextest archive, as the one CI builds does, runs the tree from it and compiles nothing; any other
# compiles the tree from the working tree.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <ngsi_ld|andromeda|etsi|annex_c|distributed|pgbouncer|interoperability>" >&2
    exit 2
fi

tree="$1"
case "$tree" in
    ngsi_ld) stack=base; binaries='ngsi_ld_*' ;;
    andromeda) stack=andromeda; binaries='andromeda_*' ;;
    etsi) stack=base; binaries=etsi ;;
    annex_c) stack=base; binaries=annex_c ;;
    distributed) stack=multi; binaries='distributed_*' ;;
    pgbouncer) stack=pgbouncer; binaries='pgbouncer_*' ;;
    interoperability) stack=interoperability; binaries=interoperability ;;
    *) echo "unknown tree: ${tree}" >&2; exit 2 ;;
esac
archive=/andromeda-test-suite.tar.zst

root="$(git rev-parse --show-toplevel)"
cd "$root"
compose=(docker compose -f "tools/andromeda-test-suite/stacks/${stack}/docker-compose.yaml")
report_dir="${REPORT_DIR:-target/suite-reports}"

trap '"${compose[@]}" --profile runner rm --stop --force -v; "${compose[@]}" down --remove-orphans' EXIT

if [ -n "${ANDROMEDA_IMAGE:-}" ]; then export ANDROMEDA_PULL_POLICY=always; else export ANDROMEDA_PULL_POLICY=build; fi
if [ -n "${ANDROMEDA_POSTGRES_IMAGE:-}" ]; then
    export ANDROMEDA_POSTGRES_PULL_POLICY=always
else
    export ANDROMEDA_POSTGRES_PULL_POLICY=build
fi
if [ -n "${ANDROMEDA_TEST_SUITE_RUNNER_IMAGE:-}" ]; then
    "${compose[@]}" --profile runner pull runner
else
    "${compose[@]}" --profile runner build runner
fi
"${compose[@]}" up --detach --wait

nextest=(cargo nextest run --tool-config-file runner:/andromeda/tools/andromeda-test-suite/stacks/runner/nextest.toml --profile suite-verdict)
if "${compose[@]}" run --rm --no-deps runner test -f "$archive"; then
    nextest+=(--archive-file "$archive" --workspace-remap /andromeda -E "binary_id(#andromeda-test-suite::${binaries})")
else
    nextest+=(-p andromeda-test-suite --test "$binaries")
fi

status=0
# The stack is up and healthy. Without --no-deps, run starts the services it depends on again, so a locally built image
# is rebuilt and every container from it recreated under a new address, which a proxy that resolved the old one keeps.
"${compose[@]}" run --rm --no-deps runner "${nextest[@]}" || status=$?

mkdir -p "$report_dir"
copy_status=0
"${compose[@]}" run --rm --no-deps runner cat /target/nextest/suite-verdict/junit.xml > "${report_dir}/${tree}.junit.xml" || copy_status=$?
if [ "$status" -eq 0 ]; then
    status="$copy_status"
fi
exit "$status"
