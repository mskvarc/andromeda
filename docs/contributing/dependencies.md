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

When `cargo deny` reports an advisory that doesn't affect the broker, ignore it in `deny.toml` with its id and a reason that says why the broker can't reach the affected code. Scanners report the same advisory against the broker image, so every ignore also gets statements in `.vex/andromeda.openvex.json`, an unmaintained crate's included. Docker Scout reads the Rust advisory database and reports every advisory `cargo deny` does. Trivy and Grype read GitHub's advisory database alone, which lists some of them.

- The advisory gets one statement under each of its identifiers, its RustSec id, its CVE id and its GitHub id, because Docker Scout applies only a statement named by the identifier it reports. Each lists the advisory's other identifiers as aliases, and the statements are otherwise the same.
- The status is `not_affected`, with the OpenVEX justification that fits. `vulnerable_code_not_in_execute_path` says the affected code never runs, and `vulnerable_code_not_present` says there is none, as for an advisory that reports an unmaintained crate.
- The impact statement is the reason of the ignore, word for word. When the justification rests on what the crates linking the affected version call, as `vulnerable_code_not_in_execute_path` and `vulnerable_code_cannot_be_controlled_by_adversary` do, it names each of them with its version, such as `rumqttc 0.25.1`.
- The statement names the image twice, as `pkg:oci/andromeda?repository_url=ghcr.io/<owner>/andromeda`, which Trivy and Grype match, and as `pkg:docker/<owner>/andromeda?repository_url=ghcr.io`, which Docker Scout matches. `<owner>` is the owner of the repository `[workspace.package]` names, and neither carries a version. The affected crate is the subcomponent of both, as `pkg:cargo/<name>@<version>` at the version `Cargo.lock` holds.
- The statement carries its own `timestamp`, the time its ruling was known to hold, so a later change elsewhere in the document leaves its date as it is.

Change a statement in place, never by adding another one under the same identifier. Whenever a statement changes, set its `timestamp` and the document's `last_updated`, and raise the document's `version`. The gate `advisory_rulings` fails on an ignore without a statement. It also fails on a statement whose identifiers `deny.toml` doesn't ignore, whose impact statement differs from the reason, whose alias names no statement or one with other identifiers, another justification or other crates, that leaves out a form of the image or has no timestamp, or whose crate version the lockfile doesn't hold. Last, it fails when the impact statement of a ruling that rests on the linking crates leaves out a crate the lockfile has linking that version. That one fires when a dependency update adds such a crate or moves one to another version, and the ruling then needs a new proof, as [Advisory rulings](../advisory-rulings.md#redoing-a-proof) describes. Once the broker no longer links the affected version, the gate fails on the crate version the lockfile no longer holds, and the ignore and its statements go together. [Release process](../release-process.md#advisories-in-the-image) describes how the document reaches the image.

## Code from other projects

When you copy or adapt code from another project, keep its licence. Say at the top of each such file where the code comes from, put a copy of its licence file beside it, and add an entry for it to the [Notice](../../NOTICE.md). Code under a licence that isn't compatible with the EUPL-1.2 can't come in.

## Package metadata

Every package inherits every `[workspace.package]` field, including `license` unless the package carries code under a further licence. It also sets `[lints] workspace = true` and declares a `description` of its own. Lint levels are set only in `[workspace.lints]`, never with `#![deny]` or `#![forbid]` in a crate root. The gate `package_metadata` checks all of this.
