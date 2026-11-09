# Continuous integration

## Workflows

| Workflow | Runs on | What it runs |
|---|---|---|
| `ci` | every pull request that isn't a draft, every push to `main`, on demand | one job per check and the `pr-ok` gate, and except on a pull request the `ngsi_ld`, `etsi` and `annex_c` suite trees and the `ci-ok` gate |
| `suite` | weekly, on demand | the `andromeda`, `distributed` and `interoperability` trees, or any single tree except `pgbouncer` picked when it's started by hand, through `suite-trees` |
| `suite-trees` | called by `ci` and `suite` | the arm64 broker image, the runner image and the trees the caller names, so both workflows run a tree the same way |
| `advisories` | weekly, on demand | `cargo deny check advisories`, for advisories published while nothing is being committed |
| `postgresql` | pushes to `main` that change `docker/images/postgresql` or the workflow itself, weekly, on demand | builds, checks with [PostgreSQL image checks](testing/postgresql-image-checks.md), publishes and signs the PostgreSQL image |
| `release` | version tags | the release, as [Release process](release-process.md) describes |
| `prune-ci-images` | daily, on demand | deletes the `broker-<sha>`, `broker-amd64-<sha>` and `runner-<sha>` tags in `andromeda-ci` once they are 30 days old, except those of the commit at the head of `main`, a `broker-input-<key>`, `broker-amd64-input-<key>` or `runner-input-<key>` image once no commit tag left points at it, and untagged versions in `andromeda-ci`, `andromeda-buildcache` and `postgresql-buildcache` once they are a day old and no commit tag left points at them. It reads what a commit tag points at from the tag that names the digest, so it fetches no manifest, reads it again just before an input image or an untagged version goes, and prunes each package in a job of its own. The published `andromeda` and `postgresql` packages are left alone |

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
| `trees / broker-image` | gives the commit its arm64 broker image in `ghcr.io/<owner>/andromeda-ci` as `broker-<sha>`, without building it when the commit's broker inputs already have one. A pull request skips it |
| `broker-image (linux/amd64)` | gives the commit its amd64 broker image the same way, on an amd64 runner, as `broker-amd64-<sha>`, from `broker-amd64-input-<key>`. It runs beside `trees`, and no gate or suite tree waits for it. A release publishes it, so a release of a commit that has it builds nothing. A pull request skips it |
| `trees / runner-image` | gives the commit its arm64 runner image, with every tree's test binaries compiled into it, in `ghcr.io/<owner>/andromeda-ci` as `runner-<sha>`, without building it when the commit's suite inputs already have one. It runs beside `trees / broker-image`. A pull request skips it |
| `trees / suite (<tree>)` | one job for each of the `ngsi_ld`, `etsi` and `annex_c` trees, through `run-suite.sh` as [Test suite](testing/test-suite.md) describes, with the commit's broker and runner images and the published `postgresql:tsl` image. Each runs its tests from the runner image's archive and compiles nothing. A pull request skips them |

The broker and runner images are named by the key of their inputs, a SHA-256 over the path, mode and content of every file the image is built from. The inputs are the directories the image compiles from its package, `src/` for the broker and `src/` and `tests/` for the runner, every workspace crate it compiles through normal and build dependencies without that crate's `tests/`, `benches/` and `examples/`, the manifest of every workspace package, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/` and the image's own paths. For the broker those are `migrations/`, `docker/images/andromeda/` and `LICENSE.md`, and for the runner `tools/andromeda-test-suite/stacks/runner/Dockerfile`. The key also covers the actions that build the image, `image-inputs` with `keyed-broker-image` and `broker-image`, or with `keyed-runner-image`, because they set the platform, the build target and the labels, though the build context leaves them out. The job computes the key without building anything and looks for `broker-input-<key>`, `broker-amd64-input-<key>` or `runner-input-<key>`. Only when it's missing does it build the image, from a build context that holds the inputs alone, with an empty file standing in for each library, binary and build script of a workspace member the package doesn't compile. A lookup that fails for any other reason fails the job, so a registry error never builds an image again. The image carries no commit. `broker-<sha>`, `broker-amd64-<sha>` and `runner-<sha>` are indexes over it with the commit as their `org.opencontainers.image.revision` annotation, so they share every layer with the image. Each index is also tagged `<name>-<sha>-<digest>`, the digest of the image it points at without its `sha256:` prefix. A commit that changes only documentation or workflows builds no image, and one that changes only the suite builds only the runner.

The runner image compiles the suite in the `suite` stage of `tools/andromeda-test-suite/stacks/runner/Dockerfile`. The suite's third-party dependencies come from a cargo-chef layer that stays while the manifests, the lockfile and the toolchain do, so a change to the suite alone compiles the suite and `andromeda-postgresql-seaquery`, the one other workspace crate it uses.

When the broker image is built, in `ci` or in a release, sccache takes every library crate compiled before from the Actions cache and stores the ones it compiles, so a change to one workspace crate or one dependency compiles that crate and the crates that depend on it. Each compile step of the build log ends with sccache's hit and miss counts. Binaries, build scripts and procedural macros are linked, which sccache can't cache, so they compile every time. The Actions cache holds 10 GB for the repository, shared with the build caches of `clippy`, `unit` and `sql` and the recorded passes, and GitHub evicts the least recently used entries beyond that, so the newest build's entries stay.

`clippy`, `unit` and `sql` each keep one build in the Actions cache, the cargo registry and the whole target directory with the workspace crates, saved by the newest run on `main` that wasn't cancelled. A run saves its build under a key of its own and then deletes the job's earlier one. A pull request restores it and saves nothing. The jobs run cargo with checksum freshness, so a restored crate is fresh when the content of every file it was compiled from is unchanged, whatever the files' modification times, and a crate whose file changed is compiled again with the crates that depend on it. A build script's `rerun-if-changed` paths are still compared by modification time, so after the restore every tracked file is dated 2000 except those that differ from the last commit the job passed on, which are dated now. A build script then reruns only when one of its inputs changed. A run that fails still saves what it compiled, but keeps the commit its restored build recorded, because the failure can stop the build before a build script whose input changed. A change to the toolchain or `.cargo/config.toml` starts the job's build from nothing. A change to `Cargo.lock` or a manifest restores the build of the earlier dependency state without its workspace crates, so the third-party crates the change leaves alone stay compiled. A run on `main` is the exception, because it saves its build. It compiles a new dependency state from nothing, so the saved build holds that state's crates alone and doesn't grow with every dependency change.

The `suite` workflow calls `suite-trees` with its own trees, as `ci` does. A weekly run of a commit `ci` already gave its images builds no image, and every tree runs from the runner image's archive. A change to `docker/images/postgresql` reaches the suite trees once the `postgresql` workflow has published it from `main`.

When a job runs its checks as separate steps, as `unit` and `sql` do, every step runs whatever an earlier one reported, so one run lists every failure. [Getting started](contributing/getting-started.md) lists the commands that run the same checks locally.

## Recorded passes

Every job of `ci` except `deny` and the image jobs, and every tree of `suite`, first computes a key over everything it reads, without building anything. When a run on `main` recorded a pass under that key, the job passes without running its checks, and a notice in its log names the run that recorded it. Only a job that runs its checks on `main` and passes records its key, as an Actions cache entry named `verdict-<job>-<key>`. A pull request reuses a pass but never records one, and a failed or cancelled job records nothing. A pass the cache has evicted is run again. `deny` runs on every commit, because its advisory check reads the day's advisory database rather than the commit.

A job's inputs are every tracked file outside `.github/` that its exclusions don't name, so a new file is an input until it is excluded. Every workflow under `.github/workflows/` and every action under `.github/actions/` are inputs of every job, so a change to any of them runs every job again once.

- `clippy` and `unit` exclude the documentation pages, every `*.md` and `docs/assets/`, and the image and stack definitions, `docker/`, `tools/andromeda-test-suite/stacks/` and `.dockerignore`. The generated configuration template, schema and descriptor stay inputs, because `unit` compares them with what the code generates.
- `sql` excludes what `clippy` does, and also the broker's `src/` and `tools/andromeda-test-suite/`. Its key also covers `migrations/seal-ledger.toml` as the base holds it, the branch a pull request merges into or the parent of a push, because `verify --against` compares the working copy with that ledger.
- `fmt` also excludes JSON, JSON-LD and ABNF files, `docs/configuration/`, `migrations/`, the SQL formatter and linter settings, shell scripts and the settings of the other tools. That leaves the Rust sources, the manifests, the lockfile, `rust-toolchain.toml`, `.cargo/` and `rustfmt.toml`.
- `machete` excludes what `fmt` does and `rustfmt.toml`.
- `taplo` and `shellcheck` pick their files by extension, as the tools do. `taplo` reads every TOML file and the configuration schema, and `shellcheck` every `*.sh`.
- A suite tree's key covers the keys of the broker and runner image inputs, `tools/andromeda-test-suite/stacks/`, `tools/andromeda-test-suite/run-suite.sh`, `.config/nextest.toml` and the digest of the PostgreSQL image, which the tree then runs by that digest.

So a push that changes only documentation reuses every job except `deny`, and the image jobs find all three images already built. A pass of `sql` is reused only when the base's ledger matches as well, so a pull request that brings back the migrations of an earlier commit on `main`, as a revert of a seal does, still runs `verify --against` its base.

Starting `ci` or `suite` by hand with `fresh` set ignores every recorded pass and runs each check in full. It still reuses an image whose inputs already have one, so an input tag stays on the manifest the commit tags point at.

## Gates

- `pr-ok` passes when `fmt`, `taplo`, `machete`, `deny`, `shellcheck`, `clippy`, `unit` and `sql` all passed. A merge into `main` requires it.
- `ci-ok` passes when `pr-ok` and every suite tree passed. It runs on a push to `main` and on a run started by hand, never on a pull request. A release requires it on the tagged commit.

So the suite trees don't gate a merge, and a tree that a change breaks turns red only once the change is on `main`. Run the trees your change touches before you merge it, with `run-suite.sh` as [Test suite](testing/test-suite.md) describes. A red tree on `main` is the next thing to fix, because no later commit can be released until it's fixed.
