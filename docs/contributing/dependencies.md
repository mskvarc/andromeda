# Dependencies

## Prefer an existing crate

Before you implement new functionality, look for a well-maintained crate that already covers it, judging by its downloads, its last release and its open issues. Use it unless it is unsound, abandoned, under an incompatible licence or missing something critical. Say in the pull request which crate you chose, or why none fit.

## Declaring dependencies

- Declare a third-party dependency inline in the crate that uses it, with its own version and features. It never goes in `[workspace.dependencies]` and is never written with `workspace = true`. Two crates that use the same dependency each declare it.
- Crates from this repository, under `crates/` and `tools/`, work the other way round. Each is declared once in the root `[workspace.dependencies]` with its `path`, and its consumers write `andromeda-foo.workspace = true`.
- Write versions as full semver, such as `1.2.3`, never `1` or `1.2`.
- Keep dependencies in alphabetical order, `[workspace.dependencies]` included.

The gate `manifest_hygiene` checks the version precision, the ordering and where each kind of dependency is declared. CI also fails on a dependency nothing uses, which `cargo machete --with-metadata` finds.

## Ignoring an advisory

When `cargo deny` reports an advisory that doesn't affect the broker, ignore it in `deny.toml` with its id and a reason that says why the broker can't reach the affected code. If GitHub's advisory database lists the advisory, scanners report it against the broker image as well, because Trivy reads Rust advisories from that database alone. The advisory then also gets a statement in `.vex/andromeda.openvex.json`. An advisory the database doesn't list yet can get its statement in advance, as RUSTSEC-2026-0382 has.

- The vulnerability is named by the advisory's id, with its CVE and GitHub ids as aliases.
- The status is `not_affected`, with the OpenVEX justification that fits, such as `vulnerable_code_not_in_execute_path`.
- The impact statement is the reason of the ignore, word for word. It names each crate that links the affected version, with its version, such as `rumqttc 0.25.1`, because the ruling rests on what those crates call.
- The product is the image's package URL, `pkg:oci/andromeda?repository_url=ghcr.io/<owner>/andromeda` for the owner of the repository `[workspace.package]` names, with no version. The affected crate is its subcomponent, as `pkg:cargo/<name>@<version>` at the version `Cargo.lock` holds.

Raise the document's `version` and set its `timestamp` whenever a statement changes. The gate `advisory_rulings` fails on a statement whose advisory `deny.toml` doesn't ignore, whose impact statement differs from the reason, whose crate version the lockfile doesn't hold, or whose impact statement leaves out a crate the lockfile has linking that version. The last one fires when a dependency update adds such a crate or moves one to another version, and the ruling then needs a new proof, as [Advisory rulings](../advisory-rulings.md#redoing-a-proof) describes. An ignore without a statement passes, since only the advisory databases say whether a scanner reports it. Once the broker no longer links the affected version, remove the ignore and its statement together. [Release process](../release-process.md#advisories-in-the-image) describes how the document reaches the image.

## Code from other projects

When you copy or adapt code from another project, keep its licence. Say at the top of each such file where the code comes from, put a copy of its licence file beside it, and add an entry for it to the [Notice](../../NOTICE.md). Code under a licence that isn't compatible with the EUPL-1.2 can't come in.

## Package metadata

Every package inherits every `[workspace.package]` field, including `license` unless the package carries code under a further licence. It also sets `[lints] workspace = true` and declares a `description` of its own. Lint levels are set only in `[workspace.lints]`, never with `#![deny]` or `#![forbid]` in a crate root. The gate `package_metadata` checks all of this.
