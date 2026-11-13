# Release process

A release is cut by pushing a tag, `vX.Y.Z` for a stable release or `vX.Y.Z-alpha.N`, `-beta.N` or `-rc.N` for a pre-release. The `release` workflow then publishes the signed broker image and a GitHub release with the configuration schema and descriptor attached. Before it builds anything, it refuses a tag whose commit has no passing `ci-ok` check, and a tag whose migrations break the seal. The arm64 image it publishes is the one `ci` built for the tagged commit and ran the suite trees against. The amd64 image is the one `ci` built from the same broker inputs, so a release compiles nothing when both exist, and builds the amd64 image only when `ci` has none. The seal check takes the SQL tool as the `sql` job of `ci` last compiled it on `main`. `prune-ci-images` deletes the commit's `broker-<sha>` tag 30 days after `ci` made it unless the commit is still the head of `main`, so any other commit older than that has to go through `ci` again before it can be tagged. When its broker image still exists, `ci` only tags it again.

## Stable release

1. On the release commit, with every migration the release ships in place, run `cargo run -p andromeda-sql -- seal vX.Y.Z`. It writes the release's section into `migrations/seal-ledger.toml`, creating the file on the first seal.
2. Freeze every compact code the release ships. Run `cargo nextest run -p andromeda-sql -E 'test(compact_code_freeze)'`, which fails once for each registry in `crates/andromeda-postgresql-codes/src/` and gives the `frozen_at`, `frozen_prefix` and `frozen_digest` that freeze every code the registry assigns. Set those three values in each registry it names and run it again until it passes. [Migration sealing](database/migration-sealing.md#compact-codes) explains why.
3. Write the release notes to a Markdown file outside the repository, here `../vX.Y.Z.md`. [Release notes](#release-notes) says what goes in it.
4. Check that `git cliff --bumped-version` prints `vX.Y.Z`. It reads the commits since the last stable release and calls for a major release when one of them is breaking, a minor release when one adds a feature, and a patch release otherwise.
5. Write the changelog with `GIT_CLIFF_TAG=vX.Y.Z git cliff --with-tag-message "$(cat ../vX.Y.Z.md)" -o CHANGELOG.md`. It rewrites the whole file. Never pass `--unreleased` or `--prepend`, which stop at the newest pre-release tag and leave out the commits before it.
6. Commit the ledger, the frozen registries and the changelog with a `chore(release):` header, which keeps the commit itself out of the changelog, and merge it to `main` the usual way, so `ci` checks the new section against the base.
7. Once `ci` has passed on that commit, tag it with `git tag -a --cleanup=verbatim -F ../vX.Y.Z.md vX.Y.Z` and push the tag. Without `--cleanup=verbatim`, git deletes every line of the message that starts with `#`, so the headings would be lost.

The seal always comes before the tag, so anything that changes between them, an edited migration or a new one, fails the release.

## Pre-release

Once `ci` has passed on the commit, tag it `vX.Y.Z-rc.N` (or `alpha.N`, `beta.N`) and push the tag. Nothing is sealed for a pre-release, so its migrations can still change until the stable release seals them. Notes are optional, and an annotated tag carries them the same way, as in `git tag -a --cleanup=verbatim -F ../vX.Y.Z-rc.N.md vX.Y.Z-rc.N`.

`CHANGELOG.md` stays as it is. It lists stable releases only, and a pre-release's changes appear under the stable release that follows it.

## Release notes

A GitHub release and its entry in `CHANGELOG.md` open with the annotated tag's message. A Changes section follows, which git-cliff builds with `cliff.toml` from the commits since the previous stable release. The GitHub release ends with the image's pull command and its manifest list digest, and a pre-release opens with a warning that its migrations are not sealed.

The tag message holds what the commits can't say. Use these headings, and leave out any that would be empty.

- `### Highlights` for the changes worth reading first, described in prose.
- `### Upgrade notes` for what an operator has to do when moving to the release.
- `### Security` for the write-up of each advisory the release fixes, with its identifiers.

Write every link as an absolute URL, because Dependabot and Renovate copy the release notes into the update pull requests of the image's users. Put `@` words such as `@context` in backticks, since a bare one mentions the GitHub account of that name.

The Changes section lists breaking changes first, then security fixes, deprecations, features, bug fixes and performance changes. The type and scope of each commit decide where it goes, as [Commits and pull requests](contributing/commits-and-pull-requests.md#in-the-changelog) describes. Refactors, documentation, dependency updates and build, CI, test, style and chore commits are internal changes. They are listed only for the release being cut, in a collapsed Internal changes block, so `CHANGELOG.md` keeps them for its newest release alone. A release of more than 600 commits links to the commit history instead, which keeps the release body under GitHub's limit of 125,000 characters.

To see the release body as the workflow will build it, run this before tagging.

```
GIT_CLIFF_TAG=vX.Y.Z git cliff "$(git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude '*-*')..HEAD" --with-tag-message "$(cat ../vX.Y.Z.md)" --strip all
```

To keep a commit out of the changelog, such as a change and its revert in the same release, add its full hash to `.cliffignore` at the root of the repository, one hash per line.

## What the release checks

The workflow runs `andromeda-sql verify --release <tag>`, with `--against` the nearest earlier `v` tag when there is one. It fails when a sealed migration has changed, or when a section the ledger held at the earlier tag has changed. For a stable tag, it also fails unless the ledger's last section is the one for that version and every migration in the tree is sealed. [Migration sealing](database/migration-sealing.md) describes the ledger and the contract it enforces.
