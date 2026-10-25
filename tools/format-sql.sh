#!/usr/bin/env bash
#
# Canonical formatting of the SQL migrations.
#
#   tools/format-sql.sh           rewrite every migration that is not canonical
#   tools/format-sql.sh --check   report drift and fail without writing
#
# Every file under migrations/ is reformatted with pg_format, driven by the
# repository-root .pg_format config plus the .pg_format_keywords and
# .pg_format_functions token lists that teach it the extension vocabulary
# (TimescaleDB, PostGIS, COMPRESSION, xid8) its built-in dictionary does not
# carry.
#
# CREATE FUNCTION statements are held out of pg_format and kept as written:
# pg_format reformats a plpgsql body as plain SQL, unwraps a body written as a
# string literal, and a "language plpgsql" clause can shift the indentation of
# earlier comments. Each statement is replaced by a placeholder statement
# selecting a string literal, which pg_format lays out like any other
# statement, and is put back verbatim afterwards.
#
# pg_format follows every statement with a blank line, the last one included,
# and no option disables it; trailing blank lines are dropped so a migration
# ends in exactly one newline.
#
# One more fixup runs afterwards: pg_format always opens a parenthesised list on a
# line of its own, even when the list is one short line, and no option disables
# it ("identity (\n    cache 1)"). Every such list is rejoined onto the line
# that opens it. A list spanning several lines is left as formatted.
#
# Tools are resolved by capability, not by name, so GNU, BSD and BusyBox
# userlands all work. SED, AWK and PG_FORMAT name a specific executable; a
# shell alias is invisible to a script, so an alias such as sed=gsed is
# expressed as SED=gsed. Without an override the first candidate on PATH that
# passes a probe of exactly the features used here is taken: gsed then sed, and
# gawk, mawk, nawk then awk.

set -euo pipefail

# Byte semantics everywhere: tolower, substr and bracket expressions must agree
# on lengths, and the list fixup relies on a raw control byte.
export LC_ALL=C

# A byte no migration contains, standing in for newline so the list fixup is a
# single-line substitution every sed dialect runs identically.
readonly SOH=$'\001'
readonly QUOTE="'"
readonly REJOIN="s/\\(${SOH}[[:blank:]]*([^${SOH}()]*)\\)/(\\1)/g"

die() {
    printf 'format-sql: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf 'usage: %s [--check]\n' "$0" >&2
    exit 2
}

# Whether $1 is a sed that runs the list fixup correctly.
probe_sed() {
    local output
    output=$(printf 'x (%s    a, b)\n' "$SOH" | "$1" -E "$REJOIN" 2>/dev/null) || return 1
    [ "$output" = 'x (a, b)' ]
}

# Whether $1 is an awk with tolower, match and POSIX character classes across
# embedded newlines.
probe_awk() {
    local output
    output=$(printf 'A\n  B\n' | "$1" '{ t = (NR == 1) ? $0 : t "\n" $0 } END { if (match(tolower(t), /a[[:space:]]+b/)) print RSTART ":" RLENGTH }' 2>/dev/null) || return 1
    [ "$output" = '1:5' ]
}

# Whether $1 is pg_format.
probe_pg_format() {
    "$1" --version >/dev/null 2>&1
}

# Print the first of the candidates that passes the probe, or the executable
# the named override variable holds; fail with MISSING when neither works.
#   resolve VARIABLE PROBE MISSING CANDIDATE...
resolve() {
    local variable=$1 probe=$2 missing=$3 candidate
    shift 3
    local override=${!variable:-}
    if [ -n "$override" ]; then
        command -v -- "$override" >/dev/null 2>&1 || die "$variable=$override is not an executable on PATH"
        "$probe" "$override" || die "$variable=$override does not support what this script needs"
        printf '%s\n' "$override"
        return
    fi
    for candidate in "$@"; do
        if command -v -- "$candidate" >/dev/null 2>&1 && "$probe" "$candidate"; then
            printf '%s\n' "$candidate"
            return
        fi
    done
    die "$missing"
}

# Replace every CREATE FUNCTION statement of stdin with a placeholder, writing
# statement N to $1/N and the statement count to $1/count.
#
# A statement runs from "create [or replace] function" at the start of a line
# to the "$$;" that follows the first "$$" after it.
shield() {
    "$AWK" -v dir="$1" -v q="$QUOTE" '
        { text = (NR == 1) ? $0 : text "\n" $0 }
        END {
            count = 0
            if (NR > 0) {
                out = ""
                rest = text
                at_line_start = 1
                while (1) {
                    prefix = at_line_start ? "\n" : ""
                    if (!match(prefix tolower(rest), /\ncreate (or replace )?function[^a-z0-9_]/)) break
                    start = RSTART + 1 - length(prefix)
                    body = index(substr(rest, start), "$$")
                    if (body == 0) break
                    body += start - 1
                    finish = index(substr(rest, body + 2), "$$;")
                    if (finish == 0) break
                    stop = body + 2 + finish + 1
                    path = dir "/" count
                    printf "%s", substr(rest, start, stop - start + 1) > path
                    close(path)
                    out = out substr(rest, 1, start - 1) "select " q "__function_statement_" count "__" q ";"
                    count++
                    rest = substr(rest, stop + 1)
                    at_line_start = 0
                }
                print out rest
            }
            print count > (dir "/count")
        }
    '
}

# Put back every statement shield wrote to $1, failing when the placeholders
# on stdin are not exactly those shield left, in order.
restore() {
    "$AWK" -v dir="$1" -v q="$QUOTE" -v count="$(cat "$1/count")" '
        function slurp(path,    line, content, first) {
            content = ""
            first = 1
            while ((getline line < path) > 0) {
                content = first ? line : content "\n" line
                first = 0
            }
            close(path)
            return content
        }
        { text = (NR == 1) ? $0 : text "\n" $0 }
        END {
            placeholder = "select[[:space:]]+" q "__function_statement_[0-9]+__" q ";"
            out = ""
            rest = text
            seen = 0
            while (match(tolower(rest), placeholder)) {
                position = substr(rest, RSTART, RLENGTH)
                sub(/^.*__function_statement_/, "", position)
                sub(/__.*$/, "", position)
                if (position + 0 != seen || seen >= count) exit 3
                out = out substr(rest, 1, RSTART - 1) slurp(dir "/" seen)
                rest = substr(rest, RSTART + RLENGTH)
                seen++
            }
            if (seen != count) exit 3
            if (NR > 0) print out rest
        }
    '
}

# Drop the blank lines pg_format leaves after the last statement, so the text
# ends in exactly one newline.
trim_trailing_blank_lines() {
    "$AWK" '
        { text = (NR == 1) ? $0 : text "\n" $0 }
        END {
            sub(/\n+$/, "", text)
            if (text != "") print text
        }
    '
}

# Rejoin every single-line parenthesised list of stdin onto the line that
# opens it. Newlines become SOH for the substitution and are mapped back after.
rejoin() {
    { tr '\n' "$SOH"; printf '\n'; } | "$SED" -E "$REJOIN" | tr -d '\n' | tr "$SOH" '\n'
}

# Write the canonical text of migration $1 to $2.
format_file() {
    local migration=$1 canonical=$2 work
    work=$(mktemp -d "$scratch/file.XXXXXX")
    if grep -q "$SOH" "$migration"; then
        die "$migration contains a 0x01 byte, which the list fixup reserves"
    fi
    shield "$work" < "$migration" > "$work/shielded"
    "$PG_FORMAT" \
        --config "$root/.pg_format" \
        --extra-keyword "$root/.pg_format_keywords" \
        --extra-function "$root/.pg_format_functions" \
        < "$work/shielded" > "$work/formatted" \
        || die "pg_format failed on $migration"
    restore "$work" < "$work/formatted" > "$work/restored" \
        || die "pg_format lost or reordered a function statement placeholder in $migration"
    trim_trailing_blank_lines < "$work/restored" | rejoin > "$canonical"
}

mode=write
case $# in
    0) ;;
    1) [ "$1" = --check ] && mode=check || usage ;;
    *) usage ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)

# resolve runs in a subshell, so its die only ends that subshell; exit here
# with the status it already reported.
SED=$(resolve SED probe_sed "no working sed found; install GNU sed or set SED" gsed sed) || exit
AWK=$(resolve AWK probe_awk "no awk with POSIX character classes found; install gawk or set AWK" gawk mawk nawk awk) || exit
PG_FORMAT=$(resolve PG_FORMAT probe_pg_format "pg_format is not installed; install pgFormatter or set PG_FORMAT" pg_format) || exit

scratch=$(mktemp -d "${TMPDIR:-/tmp}/format-sql.XXXXXX")
trap 'rm -rf "$scratch"' EXIT

drifted=0
while IFS= read -r migration; do
    canonical="$scratch/canonical"
    format_file "$migration" "$canonical"
    if cmp -s "$canonical" "$migration"; then
        continue
    fi
    case $mode in
        write)
            cat "$canonical" > "$migration"
            printf 'formatted %s\n' "$migration"
            ;;
        check)
            printf 'not formatted: %s\n' "$migration" >&2
            drifted=$((drifted + 1))
            ;;
    esac
done < <(find "$root/migrations" -type f -name '*.sql' | sort)

if [ "$drifted" -gt 0 ]; then
    die "$drifted migration(s) are not formatted; run tools/format-sql.sh to fix"
fi
