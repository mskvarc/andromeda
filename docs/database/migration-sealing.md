# Migration sealing

The migrations are the schema contract of every release. An installation that ran one release's migrations has to be able to run the next release's on top of them, and sealing turns that into something a tool can check. When a stable release is cut, `andromeda-sql seal <version>` writes the SHA-256 of every migration the release adds to `migrations/seal-ledger.toml`. Those files never change after that, and `andromeda-sql verify` checks that they haven't.

## The contract

A sealed migration keeps its exact bytes, its set directory and its file name for as long as the repository exists. Nobody edits, reformats, deletes or renames it. An installation that ran it has the schema those bytes describe and won't run it again, so any change to that schema goes in a new migration.

A new migration gets a number higher than every sealed one. The three sets share one version space and the migrator applies each set in version order. A file numbered below one that already shipped would run out of order on a fresh installation, and never run at all on an upgraded one.

Sealed sections of the ledger never change either. The ledger only grows, by one section for each release it seals.

Only stable releases (`vX.Y.Z`) are sealed. A pre-release (`alpha`, `beta`, `rc`) ships nothing a later release has to keep, so its migrations can still change until the stable release seals them. A version with build metadata isn't a release at all, and the tool refuses it.

## The ledger

The ledger has one section per stable release, in release order. Each section lists only the migrations that release added, in version order, so it is complete the moment it's written and nothing a later release does can touch it. For each migration it records the set directory, the file name and the SHA-256 of the file's committed bytes as 64 lowercase hex digits. The hash algorithm is named once at the top of the file.

```toml
# Written by `andromeda-sql seal`. A sealed migration and its section never change.

algorithm = "sha-256"

[[release]]
version = "v1.0.0"

[[release.migration]]
set = "platform"
name = "0001_tenants.sql"
sha256 = "..."
```

The tool always writes the file in one canonical form, with the header comment, key order and ordering shown above. `verify` renders the ledger it reads in that form again and fails if the text differs, so a hand edit shows up even when it only touches a comment or the order. Nobody edits the ledger by hand. If a section turns out to be wrong, it gets fixed before the release ships, by deleting the unpushed section and sealing again.

## Why raw bytes

The digest covers the bytes as committed rather than a normalised version of them, so it identifies the exact file an installation ran. Normalising first would tie the seal to the normaliser, and a new `pg_format` release or a change to the layout adjustments would change the digest of a file nobody touched.

The canonical layout can still change after a seal, because sealed files are exempt from it. `format` skips every sealed migration and `format --check` doesn't report them, so a formatter change applies only to migrations that aren't sealed yet and never rewrites history. `lint` works the same way. It reads sealed migrations as part of the schema but reports nothing in them. Otherwise a rule added after the seal could flag a file that can no longer change, and that would block every later seal.

## Sealing a release

`andromeda-sql seal v1.2.0` fails and leaves the ledger as it was unless all of these hold.

- The version is a stable `vX.Y.Z`, with no pre-release part and no build metadata.
- `pg_format` and Squawk can be found, and Squawk is release 2.70.0.
- `verify` passes, so no sealed file has changed and the ledger is in canonical form.
- The version is higher than the last sealed release.
- There is at least one unsealed migration.
- `format --check` and `lint` pass.

It then appends one section with every unsealed migration and prints each file it sealed. Sealing is part of cutting the release. Run it on the release commit, commit the ledger with it, and then tag.

## Compact codes

The broker also writes small numeric codes into `smallint` columns, such as the value shape of a history row or the status of a tenant. Each code domain is a registry in `andromeda-postgresql-codes`. Rows written with a code outlive the release that wrote them, so a code a release ships keeps its number and its label for good. A registry freezes its leading codes with three values, `frozen_at`, `frozen_prefix` and `frozen_digest`, and its own tests fail when a frozen code is renumbered, relabelled, reordered or removed. Codes after the frozen prefix haven't shipped yet and can still change.

A release ships every code its registries assign when it is sealed. So `frozen_at` names the release the freeze was last brought up to, and it always has to be the last release in the ledger. The `compact_code_freeze` test of `andromeda-sql` reads the ledger and fails for every registry that names any other release. For each one, the failure gives the three values that freeze every code the registry assigns. The test runs in the `sql` job of `ci`, so the commit that seals a release fails until every registry is frozen at it, and the release workflow refuses a tag whose commit hasn't passed `ci`.

## Verifying

`andromeda-sql verify` fails, naming the file and the broken rule, in these cases.

- A sealed migration was edited, so its bytes no longer match its digest, or it was deleted or renamed, so it isn't where it was sealed.
- An unsealed migration has a version at or below the highest sealed one.
- The ledger isn't in canonical form, lists a release twice, has a section that seals nothing, or seals the same file in two sections.

It stops with an error before any of these checks when the ledger can't be read at all, for example because of an unknown key, a pre-release version, a digest that isn't 64 lowercase hex digits or an algorithm other than `sha-256`. It does the same when a migration file is misnamed or two files hold the same version.

Checking the working copy alone can't catch a sealed migration that was edited and then given a fresh digest in the ledger, because the files and the ledger agree. `verify --against <revision>` covers that case. It reads the ledger as it was at that git revision and fails unless every section there is unchanged in the working copy. Sections added since then are fine. If the revision has no ledger, there is nothing sealed to compare. If its ledger can't be read, the check stops with an error. The tool reads the revision itself, read-only, through the `gix` crate, so the check doesn't need a `git` executable and can tell a missing ledger from a missing revision.

`verify --release <tag>` checks that the tree is ready to be tagged as that release. For a stable `vX.Y.Z`, the ledger's last section has to be the one for exactly that version, and every migration in the tree has to be sealed, so a file added or changed between the seal and the tag fails the check. A pre-release tag such as `v1.2.0-rc.1` needs no section, since nothing is sealed for it.

The `ci` workflow runs `verify` on every pull request that isn't a draft, on every push to `main`, and when the workflow is started by hand, unless a run on `main` already passed it with the same inputs, the ledger at the revision it verifies against among them, as [Continuous integration](../continuous-integration.md#recorded-passes) describes. It also runs `verify --against`, against the branch a pull request merges into, or otherwise against the parent commit, when there is one. The release workflow first confirms that the tagged commit passed `ci`. It then runs `verify --release` for the pushed tag, before it builds the image, against the nearest earlier `v` tag, which can be a pre-release. When there is no earlier tag, it leaves `--against` out. [Release process](../release-process.md) describes the whole sequence.
