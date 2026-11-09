# Release process

A release is cut by pushing a tag, `vX.Y.Z` for a stable release or `vX.Y.Z-alpha.N`, `-beta.N` or `-rc.N` for a pre-release. The `release` workflow then publishes the signed broker image and a GitHub release with the configuration schema and descriptor attached. Before it builds anything, it refuses a tag whose commit has no passing `ci-ok` check, and a tag whose migrations break the seal. The arm64 image it publishes is the one `ci` built for the tagged commit and ran the suite trees against. The amd64 image is the one `ci` built from the same broker inputs, so a release compiles nothing when both exist, and builds the amd64 image only when `ci` has none. The seal check takes the SQL tool as the `sql` job of `ci` last compiled it on `main`. `prune-ci-images` deletes the commit's `broker-<sha>` tag 30 days after `ci` made it unless the commit is still the head of `main`, so any other commit older than that has to go through `ci` again before it can be tagged. When its broker image still exists, `ci` only tags it again.

## Stable release

1. On the release commit, with every migration the release ships in place, run `cargo run -p andromeda-sql -- seal vX.Y.Z`. It writes the release's section into `migrations/seal-ledger.toml`, creating the file on the first seal.
2. Commit the ledger and merge it to `main` the usual way, so `ci` checks the new section against the base.
3. Once `ci` has passed on that commit, tag it `vX.Y.Z` and push the tag.

The seal always comes before the tag, so anything that changes between them, an edited migration or a new one, fails the release.

## Pre-release

Once `ci` has passed on the commit, tag it `vX.Y.Z-rc.N` (or `alpha.N`, `beta.N`) and push the tag. Nothing is sealed for a pre-release, so its migrations can still change until the stable release seals them.

## What the release checks

The workflow runs `andromeda-sql verify --release <tag>`, with `--against` the nearest earlier `v` tag when there is one. It fails when a sealed migration has changed, or when a section the ledger held at the earlier tag has changed. For a stable tag, it also fails unless the ledger's last section is the one for that version and every migration in the tree is sealed. [Migration sealing](database/migration-sealing.md) describes the ledger and the contract it enforces.
