# Continuous integration

## Workflows

| Workflow | Runs on | What it runs |
|---|---|---|
| `ci` | every pull request that isn't a draft, every push to `main`, on demand | one job per check and the `pr-ok` gate, and except on a pull request the `ngsi_ld`, `etsi` and `annex_c` suite trees and the `ci-ok` gate |
| `suite` | weekly, on demand | the `andromeda`, `distributed` and `interoperability` trees, or any single tree except `pgbouncer` picked when it's started by hand |
| `advisories` | weekly, on demand | `cargo deny check advisories`, for advisories published while nothing is being committed |
| `postgresql` | pushes to `main` that change `docker/images/postgresql` or the workflow itself, weekly, on demand | builds, checks with [PostgreSQL image checks](testing/postgresql-image-checks.md), publishes and signs the PostgreSQL image |
| `release` | version tags | the release, as [Release process](release-process.md) describes |
| `prune-ci-images` | daily, on demand | deletes the `broker-<sha>` and `runner-<sha>` tags in `andromeda-ci` once they are 30 days old, except those of the commit at the head of `main`, a `broker-input-<key>` image once no `broker-<sha>` left points at it, and untagged versions in `andromeda-ci`, `andromeda-buildcache` and `postgresql-buildcache` once they are a day old. The published `andromeda` and `postgresql` packages are left alone |

A newer commit on a pull request or on `main` cancels the run still going for the one before it, so only the newest commit of `main` gets a verdict, and a release is tagged on that commit. `[skip ci]` in a commit message skips `ci` for that push.

## Jobs of `ci`

Every check is a job of its own, so a pull request shows each one with its own status.

| Job | Runs |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `taplo` | `taplo fmt --check --diff` |
| `machete` | `cargo machete --with-metadata` |
| `deny` | `cargo deny check`, covering licences, sources, advisories and bans |
| `shellcheck` | `shellcheck` over every tracked `*.sh` |
| `clippy` | `cargo clippy` over the whole workspace, the suite included, with warnings denied |
| `unit` | the workspace unit tests, then the architecture checks, then the broker's unit tests, all under cargo-nextest |
| `sql` | `andromeda-sql` `format --check`, `lint`, `verify`, and `verify --against` the base of a pull request or the parent of a push, then the package's tests under cargo-nextest |
| `broker-image` | gives the commit its arm64 broker image in `ghcr.io/<owner>/andromeda-ci` as `broker-<sha>`, without building it when the commit's broker inputs already have one. A pull request skips it |
| `runner-image` | builds the suite's runner image once for the commit and pushes it to `ghcr.io/<owner>/andromeda-ci` as `runner-<sha>`. A pull request skips it |
| `suite (<tree>)` | one job for each of the `ngsi_ld`, `etsi` and `annex_c` trees, through `run-suite.sh` as [Test suite](testing/test-suite.md) describes, with the commit's broker and runner images and the published `postgresql:tsl` image. A pull request skips them |

The broker image is named by the key of its inputs, a SHA-256 over the path, mode and content of every file it is built from. The inputs are the sources of the broker and of every workspace crate it compiles through normal and build dependencies, the manifest of every workspace package, `Cargo.lock`, `migrations/`, `rust-toolchain.toml`, `.cargo/`, `docker/images/andromeda/`, `.dockerignore` and `LICENSE.md`. `broker-image` computes the key without building anything and looks for `broker-input-<key>`. Only when it's missing does it build the image, from a build context that holds the inputs alone, with an empty file standing in for each library, binary and build script of a workspace member the broker doesn't compile. The image carries no commit. `broker-<sha>` is an index over it with the commit as its `org.opencontainers.image.revision` annotation, so it shares every layer with the image. A commit that changes only documentation, workflows or the test suite's sources gets its tag and builds nothing.

When the broker image is built, in `ci` or in a release, sccache takes every library crate compiled before from the Actions cache and stores the ones it compiles, so a change to one workspace crate or one dependency compiles that crate and the crates that depend on it. Each compile step of the build log ends with sccache's hit and miss counts. Binaries, build scripts and procedural macros are linked, which sccache can't cache, so they compile every time. The Actions cache holds 10 GB for the repository, shared with the dependency caches of `clippy`, `unit` and `sql`, and GitHub evicts the least recently used entries beyond that, so the newest build's entries stay.

The `suite` workflow runs its trees the same way, with the published `postgresql:tsl` image and the `broker-<sha>` and `runner-<sha>` images that `broker-image` and `runner-image` gave the commit it runs. Those exist for every commit `ci` ran on outside a pull request, for thirty days or for as long as the commit is the head of `main`, so a `suite` run started by hand on another branch needs a `ci` run started by hand on that branch first. A change to `docker/images/postgresql` reaches the suite trees once the `postgresql` workflow has published it from `main`.

When a job runs its checks as separate steps, as `unit` and `sql` do, every step runs whatever an earlier one reported, so one run lists every failure. [Getting started](contributing/getting-started.md) lists the commands that run the same checks locally.

## Gates

- `pr-ok` passes when `fmt`, `taplo`, `machete`, `deny`, `shellcheck`, `clippy`, `unit` and `sql` all passed. A merge into `main` requires it.
- `ci-ok` passes when `pr-ok` and every suite tree passed. It runs on a push to `main` and on a run started by hand, never on a pull request. A release requires it on the tagged commit.

So the suite trees don't gate a merge, and a tree that a change breaks turns red only once the change is on `main`. Run the trees your change touches before you merge it, with `run-suite.sh` as [Test suite](testing/test-suite.md) describes. A red tree on `main` is the next thing to fix, because no later commit can be released until it's fixed.
