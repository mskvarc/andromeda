# Release process

A release is cut by pushing a tag, `vX.Y.Z` for a stable release or `vX.Y.Z-alpha.N`, `-beta.N` or `-rc.N` for a pre-release. The `release` workflow then publishes the signed broker image with its provenance, SBOM and VEX attestations, and a GitHub release with the configuration schema, the descriptor, the VEX document and the CycloneDX SBOM of each platform image attached. Before it builds anything, it refuses a tag whose commit has no passing `ci-ok` check, and a tag whose migrations break the seal. The arm64 image it publishes is the one `ci` built for the tagged commit and ran the suite trees against. The amd64 image is the one `ci` built from the same broker inputs, so a release compiles nothing when both exist, and builds the amd64 image only when `ci` has none. The seal check takes the SQL tool as the `sql` job of `ci` last compiled it on `main`. `prune-ci-images` deletes the commit's `broker-<sha>` tag 30 days after `ci` made it unless the commit is still the head of `main`, so any other commit older than that has to go through `ci` again before it can be tagged. When its broker image still exists, `ci` only tags it again.

## Stable release

1. On the release commit, with every migration the release ships in place, run `cargo run -p andromeda-sql -- seal vX.Y.Z`. It writes the release's section into `migrations/seal-ledger.toml`, creating the file on the first seal.
2. Freeze every compact code the release ships. Run `cargo nextest run -p andromeda-sql -E 'test(compact_code_freeze)'`, which fails once for each registry in `crates/andromeda-postgresql-stored-values/src/` and gives the `frozen_at`, `frozen_prefix` and `frozen_digest` that freeze every code the registry assigns. Set those three values in each registry it names and run it again until it passes. [Migration sealing](database/migration-sealing.md#compact-codes) explains why.
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

## Advisories in the image

The broker binary carries its own dependency tree, which `cargo auditable` embeds when the image is built, so an SBOM generator or a scanner sees every crate the image ships. The gate `auditable_build` fails when the broker image builds the binary any other way. The release attests build provenance and the OpenVEX document `.vex/andromeda.openvex.json` to the image's manifest list. It generates an SPDX and a CycloneDX SBOM of each platform image, both from one Syft scan of that image's own digest, because a manifest list resolves to the platform of whoever pulls it. The CycloneDX SBOM follows version 1.6 of the specification, whatever version Syft writes by default. Each SBOM is attested to its platform image alone, as BuildKit attaches the SBOMs it generates. A lookup by a platform image's digest, or through a tool's platform option, finds that image's two, and a lookup by tag reaches the manifest list and finds none. The VEX document is attested to each platform image as well, because a scanner may look attestations up by the digest of the platform image it pulled.

Dependency-Track reads only CycloneDX and fetches nothing from a registry. The release attaches the CycloneDX SBOM of each platform image as `andromeda-vX.Y.Z-linux-amd64.cdx.json` and `andromeda-vX.Y.Z-linux-arm64.cdx.json`, which can be uploaded to a project as they are. To take the signed copy from the registry instead, read the platform image's digest from `docker buildx imagetools inspect ghcr.io/<owner>/andromeda:X.Y.Z` and save the verified attestation's SBOM.

```
gh attestation verify oci://ghcr.io/<owner>/andromeda@<platform digest> --repo <owner>/andromeda --signer-workflow <owner>/andromeda/.github/workflows/release.yaml --predicate-type https://cyclonedx.org/bom --format json --jq '.[0].verificationResult.statement.predicate' > andromeda-linux-amd64.cdx.json
```

The VEX document tells scanners which advisories don't affect the image. It rules on every advisory `deny.toml` ignores, once under each of the advisory's identifiers, with the ignore's reason as the impact statement, and the gate `advisory_rulings` holds the two files in step, as [Dependencies](contributing/dependencies.md#ignoring-an-advisory) describes. [Advisory rulings](advisory-rulings.md) holds the proofs. Each statement names the image as `pkg:oci/andromeda?repository_url=ghcr.io/<owner>/andromeda` and as `pkg:docker/<owner>/andromeda?repository_url=ghcr.io`, both with no version, which covers every digest of the image in either form a scanner names a pulled image by. `<owner>` is the owner of the repository `[workspace.package]` names, and the gate refuses any other product, because a statement for any other name matches nothing.

An advisory published after a release gets a new VEX attestation on the image that release published.

1. On a branch off the release's tag, add the ignore and the statement, so the gate checks the statement against the lockfile the release shipped. When `main` still links the same crate version, add both to `main` too.
2. Start `ci` by hand on the branch and let it pass.
3. Start the `vex` workflow on the branch with the release's version, such as `1.0.0`. It refuses a commit without a passing `ci-ok` check, attests the branch's document to the release's manifest list and to both platform images, and replaces the release's `andromeda-vX.Y.Z.openvex.json` asset with it.

This works for releases cut after VEX attestations were introduced. `v1.0.0` predates them, so its tag holds neither the `vex` workflow nor the document, and its binary carries no dependency tree, which leaves a scanner no crate of it to report.

Each attestation carries the whole document. Trivy reads only the first OpenVEX attestation the registry lists for a digest, which may still be the older one. Docker Scout and Grype don't read the attestation at all. The release asset always holds the newest document, so a scanner given it sees every statement, as in `trivy image --vex andromeda-vX.Y.Z.openvex.json ghcr.io/<owner>/andromeda:X.Y.Z`.

To check that scanners honour the document, scan each platform image once after a release, with the release asset saved in the current directory for Docker Scout and Grype. Docker Scout trusts only VEX documents written by Docker unless `--vex-author` names the author, which is `Andromeda`, written exactly so.

```
trivy image --vex oci --show-suppressed --platform linux/amd64 ghcr.io/<owner>/andromeda:X.Y.Z
trivy image --vex oci --show-suppressed --platform linux/arm64 ghcr.io/<owner>/andromeda:X.Y.Z
docker scout cves --platform linux/amd64 --vex-author Andromeda --vex-location . registry://ghcr.io/<owner>/andromeda:X.Y.Z
docker scout cves --platform linux/arm64 --vex-author Andromeda --vex-location . registry://ghcr.io/<owner>/andromeda:X.Y.Z
grype registry:ghcr.io/<owner>/andromeda:X.Y.Z --platform linux/amd64 --vex andromeda-vX.Y.Z.openvex.json
grype registry:ghcr.io/<owner>/andromeda:X.Y.Z --platform linux/arm64 --vex andromeda-vX.Y.Z.openvex.json
```

Trivy and Grype take Rust advisories from GitHub's advisory database alone, and report each under its CVE id when the database records one and under its GitHub id otherwise. Docker Scout reads the Rust advisory database too. It reports the rustls-webpki advisories under their CVE ids, the ones GitHub's database lists under their GitHub ids as well, and the unmaintained crates under their RustSec ids. Each of those identifiers names a statement of its own, since Docker Scout matches a statement by its name alone. Trivy lists every advisory with a statement among the suppressed findings and none among the vulnerabilities, and Docker Scout and Grype leave them out. RUSTSEC-2026-0382 and the unmaintained crates have no entry in GitHub's database, so Trivy and Grype report none of them, and their statements serve Docker Scout. osv-scanner reports every advisory as well, but it reads no VEX document, so it shows all of them.

## What the release checks

The workflow runs `andromeda-sql verify --release <tag>`, with `--against` the nearest earlier `v` tag when there is one. It fails when a sealed migration has changed, or when a section the ledger held at the earlier tag has changed. For a stable tag, it also fails unless the ledger's last section is the one for that version and every migration in the tree is sealed. [Migration sealing](database/migration-sealing.md) describes the ledger and the contract it enforces.
