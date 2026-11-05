#!/usr/bin/env bash
# Checks that the `private-tenants` test group of .config/nextest.toml lists exactly the tests that work in a tenant of
# their own. A test is private when its file calls `broker::private_tenant(` or carries a `/// Tenant: private` line. For
# each profile that puts tests in the group, and each tree the group covers (ngsi_ld, andromeda, distributed), the script
# fails with `path:line` for a private test the group omits and for a listed test that is not private. A test id is the
# file stem of its source file.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

config=.config/nextest.toml
tests=tools/andromeda-test-suite/tests
trees=(ngsi_ld andromeda distributed)
marker='broker::private_tenant\(|^[[:space:]]*///[[:space:]]*Tenant:[[:space:]]*private'

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# Lines of `tree<TAB>id<TAB>path:line`, one per private test.
for tree in "${trees[@]}"; do
    while IFS= read -r file; do
        line="$(grep -nE -m1 "$marker" "$file" | cut -d: -f1)"
        printf '%s\t%s\t%s:%s\n' "$tree" "$(basename "$file" .rs)" "$file" "$line"
    done < <(grep -rlE "$marker" "$tests/$tree" | sort)
done | sort -t$'\t' -k1,1 -k2,2 > "$scratch/private.tsv"

# Lines of `profile<TAB>tree<TAB>id<TAB>config:line`, one per id a profile's private-tenants override lists.
awk -v config="$config" '
    function flush(   i) {
        if (listed) {
            for (i = 1; i <= count; i++) {
                printf "%s\t%s\t%s\t%s:%d\n", profile, tree, ids[i], config, lines[i]
            }
        }
        count = 0; listed = 0; tree = ""
    }
    /^\[/ {
        flush()
        if (match($0, /^\[\[profile\.[^.]+\.overrides\]\]$/)) {
            profile = substr($0, 11, length($0) - 10 - 12)
            inside = 1
        } else {
            inside = 0
        }
        next
    }
    inside && /^test-group = .private-tenants.$/ { listed = 1; next }
    inside {
        if (match($0, /binary\(~[a-z_]+_\)/)) {
            tree = substr($0, RSTART + 8, RLENGTH - 10)
        }
        rest = $0
        while (match(rest, /[a-z]_[a-z][a-z][a-z]_[a-z][a-z][a-z]_[0-9][0-9][0-9][0-9]/)) {
            ids[++count] = substr(rest, RSTART, RLENGTH)
            lines[count] = NR
            rest = substr(rest, RSTART + RLENGTH)
        }
    }
    END { flush() }
' "$config" | sort -t$'\t' -k1,1 -k2,2 -k3,3 > "$scratch/listed.tsv"

status=0
profiles="$(cut -f1 "$scratch/listed.tsv" | sort -u)"
if [ -z "$profiles" ]; then
    echo "${config}:1: no profile puts tests in the private-tenants group" >&2
    exit 1
fi
for profile in $profiles; do
    for tree in "${trees[@]}"; do
        awk -F'\t' -v tree="$tree" '$1 == tree { print $2 "\t" $3 }' "$scratch/private.tsv" | sort > "$scratch/want"
        awk -F'\t' -v profile="$profile" -v tree="$tree" '$1 == profile && $2 == tree { print $3 "\t" $4 }' "$scratch/listed.tsv" | sort > "$scratch/have"
        while IFS=$'\t' read -r id location; do
            echo "${location}: ${id} works in a private tenant but is missing from the private-tenants filter of profile ${profile}" >&2
            status=1
        done < <(join -t$'\t' -v1 "$scratch/want" "$scratch/have")
        while IFS=$'\t' read -r id location; do
            echo "${location}: ${id} is listed in the private-tenants filter of profile ${profile} but is not a private-tenant test of ${tree}" >&2
            status=1
        done < <(join -t$'\t' -v2 "$scratch/want" "$scratch/have")
    done
done
exit "$status"
