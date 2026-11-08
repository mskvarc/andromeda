# Dependencies

## Prefer an existing crate

Before you implement new functionality, look for a well-maintained crate that already covers it, judging by its downloads, its last release and its open issues. Use it unless it is unsound, abandoned, under an incompatible licence or missing something critical. Say in the pull request which crate you chose, or why none fit.

## Declaring dependencies

- Declare a third-party dependency inline in the crate that uses it, with its own version and features. It never goes in `[workspace.dependencies]` and is never written with `workspace = true`. Two crates that use the same dependency each declare it.
- Crates from this repository, under `crates/` and `tools/`, work the other way round. Each is declared once in the root `[workspace.dependencies]` with its `path`, and its consumers write `andromeda-foo.workspace = true`.
- Write versions as full semver, such as `1.2.3`, never `1` or `1.2`.
- Keep dependencies in alphabetical order, `[workspace.dependencies]` included.

The gate `manifest_hygiene` checks the version precision, the ordering and where each kind of dependency is declared. CI also fails on a dependency nothing uses, which `cargo machete --with-metadata` finds.

## Code from other projects

When you copy or adapt code from another project, keep its licence. Say at the top of each such file where the code comes from, put a copy of its licence file beside it, and add an entry for it to the [Notice](../../NOTICE.md). Code under a licence that isn't compatible with the EUPL-1.2 can't come in.

## Package metadata

Every package inherits every `[workspace.package]` field, including `license` unless the package carries code under a further licence. It also sets `[lints] workspace = true` and declares a `description` of its own. Lint levels are set only in `[workspace.lints]`, never with `#![deny]` or `#![forbid]` in a crate root. The gate `package_metadata` checks all of this.
