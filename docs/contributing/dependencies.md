# Dependencies

## Prefer an existing crate

Before you implement new functionality, look for a well-maintained crate that already covers it, judging by its downloads, its last release and its open issues. Use it unless it is unsound, abandoned, under an incompatible licence or missing something critical. Say in the pull request which crate you chose, or why none fit.

## Declaring dependencies

- Declare a third-party dependency inline in the crate that uses it, with its own version and features. It never goes in `[workspace.dependencies]` and is never written with `workspace = true`. Two crates that use the same dependency each declare it.
- Crates from this repository, under `crates/` and `tools/`, work the other way round. Each is declared once in the root `[workspace.dependencies]` with its `path`, and its consumers write `andromeda-foo.workspace = true`.
- Write versions as full semver, such as `1.2.3`, never `1` or `1.2`.
- Keep dependencies in alphabetical order, `[workspace.dependencies]` included.

The gate `manifest_hygiene` checks the version precision, the ordering and where each kind of dependency is declared. CI also fails on a dependency nothing uses, which `cargo machete --with-metadata` finds.

## Licences

`cargo deny` accepts a dependency only under a licence the `allow` list of `deny.toml` names, or under an exception `[[licenses.exceptions]]` makes for that one crate. The broker image ships the licence text of every crate its binary links, as `/usr/share/doc/andromeda/third-party-licenses.md`, beside `LICENSE.md` and `NOTICE.md`. cargo-about writes it when the image is built, from `about.toml` and the template `docker/images/andromeda/third-party-licenses.hbs`, and the build appends the `NOTICE` file of each linked crate that ships one, which cargo-about doesn't collect.

`about.toml` accepts the licences `deny.toml` allows, in the order cargo-about prefers one when a crate offers a choice. A crate `deny.toml` makes an exception for gets the same licence in `about.toml` once the binary links it, since cargo-about fails the image build on a licence it doesn't accept. The gate `licence_policy` fails when the two lists name different licences, or when `about.toml` grants a crate a licence `deny.toml` makes no exception for.

## Ignoring an advisory

When `cargo deny` reports an advisory that doesn't affect the broker, ignore it in `deny.toml` with its id and a reason that says why the broker can't reach the affected code. Scanners report the same advisory against the broker image, so every ignore also gets a ruling in `.vex/rulings.toml`, an unmaintained crate's included, and the VEX document `.vex/andromeda.openvex.json` is generated from the two. Docker Scout reads the Rust advisory database and reports every advisory `cargo deny` does. Trivy and Grype read GitHub's advisory database alone, which lists some of them.

- The ruling is a table named by the id the ignore names. `aliases` lists the advisory's other identifiers, its CVE id and its GitHub id, as the advisory gives them. The document states the advisory once under each identifier, with the others as its aliases, because Docker Scout applies only a statement named by the identifier it reports.
- `justification` is the OpenVEX justification that fits, and the status is always `not_affected`. `vulnerable_code_not_in_execute_path` says the affected code never runs, `vulnerable_code_not_present` says there is none, as for an advisory that reports an unmaintained crate, and `component_not_present` says none of the crate's code is in the binary, as for a crate only procedural macros use.
- `crates` names the crate versions the ruling is about as `<name>@<version>`, at the version `Cargo.lock` holds, such as `rustls-webpki@0.102.8`.
- The reason of the ignore is the impact statement of each of the advisory's statements. When the justification rests on what the crates linking the affected version call, as `vulnerable_code_not_in_execute_path` and `vulnerable_code_cannot_be_controlled_by_adversary` do, it names each of them with its version, such as `rumqttc 0.25.1`. When the ruling relies on a fixed release of a crate the broker uses instead, the reason names the first release that fixes the advisory as `<name> <version> or later`, such as `rustls-webpki 0.103.16 or later`, so a dependency update that stays at or above it leaves the reason true.
- Every statement names the image twice, as `pkg:oci/andromeda?repository_url=ghcr.io/<owner>/andromeda`, which Trivy and Grype match, and as `pkg:docker/<owner>/andromeda?repository_url=ghcr.io`, which Docker Scout matches. `<owner>` is the owner of the repository `[workspace.package]` names. Neither carries a version or a qualifier other than `repository_url`, because one such as `arch` narrows the product to some of the image's digests. The ruling's crates are the subcomponents of both, as `pkg:cargo/<name>@<version>`.

After changing an ignore or a ruling, run the gate with `ANDROMEDA_REGENERATE=1`, as `ANDROMEDA_REGENERATE=1 cargo nextest run -p andromeda-broker-composition --all-features -E 'test(the_vex_document_is_generated_from_the_advisory_rulings)'`, and check in the document it writes. A statement that reads as before keeps its `timestamp`, a changed or new one is dated when it is generated, and any change raises the document's `version` and moves its `last_updated`. The gate `advisory_rulings` fails on a checked-in document that differs from the generated one. It also fails on an ignore without a ruling or a reason, a ruling `deny.toml` doesn't ignore, an identifier two rulings name, and a crate version the lockfile doesn't hold. It fails when the lockfile holds a crate a reason relies on as `<name> <version> or later` at no version that high, or below it at a version the ruling isn't about, because the ignore covers every version of the crate and `cargo deny` reports nothing for it. Last, it fails when the reason of a ruling that rests on the linking crates leaves out a crate the lockfile has linking that version. That one fires when a dependency update adds such a crate or moves one to another version, and the ruling then needs a new proof, as [Advisory rulings](../advisory-rulings.md#redoing-a-proof) describes. Once the broker no longer links the affected version, the gate fails on the crate version the lockfile no longer holds, and the ignore and its ruling go together. [Release process](../release-process.md#advisories-in-the-image) describes how the document reaches the image.

## Code from other projects

When you copy or adapt code from another project, keep its licence. Say at the top of each such file where the code comes from, put a copy of its licence file beside it, and add an entry for it to the [Notice](../../NOTICE.md). Code under a licence that isn't compatible with the EUPL-1.2 can't come in.

## Package metadata

Every package inherits every `[workspace.package]` field, including `license` unless the package carries code under a further licence. It also sets `[lints] workspace = true` and declares a `description` of its own. Lint levels are set only in `[workspace.lints]`, never with `#![deny]` or `#![forbid]` in a crate root. The gate `package_metadata` checks all of this.
