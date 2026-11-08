#!/usr/bin/env bash
# Runs one tree of andromeda-test-suite against the stack it needs, with the broker and database images rebuilt from the
# working tree, and copies the tree's JUnit report to REPORT_DIR (default target/suite-reports). The stack is taken down
# afterwards with its databases, keeping the shared test-suite-target and test-suite-cargo-registry volumes. Two runs on
# one Docker host cannot overlap: every stack has a fixed project name and shares those volumes.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <ngsi_ld|andromeda|etsi|annex_c|distributed|pgbouncer|interoperability>" >&2
    exit 2
fi

tree="$1"
case "$tree" in
    ngsi_ld) stack=base; selection=(--test 'ngsi_ld_*') ;;
    andromeda) stack=andromeda; selection=(--test 'andromeda_*') ;;
    etsi) stack=base; selection=(--test etsi) ;;
    annex_c) stack=base; selection=(--test annex_c) ;;
    distributed) stack=multi; selection=(--test 'distributed_*') ;;
    pgbouncer) stack=pgbouncer; selection=(--test 'pgbouncer_*') ;;
    interoperability) stack=interoperability; selection=(--test interoperability) ;;
    *) echo "unknown tree: ${tree}" >&2; exit 2 ;;
esac

root="$(git rev-parse --show-toplevel)"
cd "$root"
compose=(docker compose -f "tools/andromeda-test-suite/stacks/${stack}/docker-compose.yaml")
report_dir="${REPORT_DIR:-target/suite-reports}"

trap '"${compose[@]}" --profile runner rm --stop --force -v; "${compose[@]}" down --remove-orphans' EXIT

"${compose[@]}" --profile runner build runner
"${compose[@]}" up --detach --wait --build

status=0
"${compose[@]}" run --rm runner cargo nextest run --profile suite-verdict -p andromeda-test-suite "${selection[@]}" || status=$?

mkdir -p "$report_dir"
copy_status=0
"${compose[@]}" run --rm --no-deps runner cat /target/nextest/suite-verdict/junit.xml > "${report_dir}/${tree}.junit.xml" || copy_status=$?
if [ "$status" -eq 0 ]; then
    status="$copy_status"
fi
exit "$status"
