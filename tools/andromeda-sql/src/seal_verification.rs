//! The checks that hold the migrations to the ledger, and the ledger to its
//! form and to an earlier revision of itself.

use crate::{
    file_digest::FileDigest,
    git_revision::GitRevision,
    migration_file::MigrationFile,
    release_tag::ReleaseTag,
    release_version::ReleaseVersion,
    seal_ledger::{SealLedger, SealLedgerError, sealed_release::SealedMigration},
    seal_violation::SealViolation,
};
use std::collections::BTreeSet;

/// Every way `migrations`, each with its raw bytes, and the ledger, read as
/// `ledger` from `ledger_text` when the file exists, break the seal.
///
/// # Errors
///
/// Returns [`SealLedgerError::Unrendered`] when the ledger's canonical form
/// cannot be rendered.
pub fn violations(ledger_text: Option<&str>, ledger: &SealLedger, migrations: &[(MigrationFile, Vec<u8>)]) -> Result<Vec<SealViolation>, SealLedgerError> {
    let mut found = Vec::new();
    if let Some(text) = ledger_text
        && ledger.render()? != text
    {
        found.push(SealViolation::LedgerNotCanonical);
    }
    found.extend(structure_violations(ledger));
    for (release, migration) in ledger.sealed_migrations() {
        // clone: each violation names the release and file it concerns
        match migrations.iter().find(|(file, _)| migration.is_of(file)) {
            None => found.push(SealViolation::SealedFileMissing {
                migration: migration.clone(),
                release: release.version().clone(),
            }),
            Some((file, bytes)) => {
                let digest = FileDigest::of(bytes);
                if digest != migration.sha256() {
                    found.push(SealViolation::SealedFileChanged {
                        file: file.clone(),
                        release: release.version().clone(),
                        sealed: migration.sha256(),
                        found: digest,
                    });
                }
            }
        }
    }
    if let Some(highest) = ledger.highest_sealed_version() {
        found.extend(
            migrations
                .iter()
                .filter(|(file, _)| file.version() <= highest && !ledger.seals(file))
                .map(|(file, _)| SealViolation::InsertedBehindSeal { file: file.clone(), highest }),
        );
    }
    Ok(found)
}

/// The sections of `ledger` that repeat a release, repeat a file or seal
/// nothing.
fn structure_violations(ledger: &SealLedger) -> Vec<SealViolation> {
    let mut found = Vec::new();
    let mut releases = BTreeSet::new();
    for release in ledger.releases() {
        if !releases.insert(release.version()) {
            found.push(SealViolation::ReleaseSealedTwice(release.version().clone()));
        }
        if release.migrations().is_empty() {
            found.push(SealViolation::EmptyRelease(release.version().clone()));
        }
    }
    let mut first_seals: Vec<(&ReleaseVersion, &SealedMigration)> = Vec::new();
    for (release, migration) in ledger.sealed_migrations() {
        match first_seals.iter().find(|(_, earlier)| earlier.is_same_file(migration)) {
            // clone: each violation names the releases and file it concerns
            Some((first, _)) => found.push(SealViolation::MigrationSealedTwice {
                migration: migration.clone(),
                first: (*first).clone(),
                second: release.version().clone(),
            }),
            None => first_seals.push((release.version(), migration)),
        }
    }
    found
}

/// Every way `ledger` and `migrations` fall short of what tagging `tag` needs.
///
/// A stable tag needs its own section as the last one, and every migration
/// sealed, so no file ships that a later release could still change. A
/// pre-release needs nothing beyond the seal itself.
#[must_use]
pub fn release_violations(ledger: &SealLedger, migrations: &[(MigrationFile, Vec<u8>)], tag: &ReleaseTag) -> Vec<SealViolation> {
    let release = match tag {
        ReleaseTag::Stable(release) => release,
        ReleaseTag::PreRelease(_) => return Vec::new(),
    };
    let mut found = Vec::new();
    // clone: each violation names the release it concerns
    match ledger.latest_release() {
        Some(latest) if latest.version() == release => {}
        Some(latest) if ledger.releases().iter().any(|sealed| sealed.version() == release) => found.push(SealViolation::ReleaseNotLatest {
            release: release.clone(),
            latest: latest.version().clone(),
        }),
        Some(_) | None => found.push(SealViolation::ReleaseNotSealed(release.clone())),
    }
    found.extend(
        migrations
            .iter()
            .filter(|(file, _)| !ledger.seals(file))
            .map(|(file, _)| SealViolation::UnsealedAtRelease {
                file: file.clone(),
                release: release.clone(),
            }),
    );
    found
}

/// Every section `earlier`, the ledger at `revision`, holds that `ledger` no
/// longer holds unchanged.
#[must_use]
pub fn changed_since(ledger: &SealLedger, earlier: &SealLedger, revision: &GitRevision) -> Vec<SealViolation> {
    earlier
        .releases()
        .iter()
        .filter_map(|sealed| {
            let now = ledger.releases().iter().find(|release| release.version() == sealed.version());
            // clone: each violation names the release and revision it concerns
            match now {
                None => Some(SealViolation::ReleaseRemovedSince {
                    release: sealed.version().clone(),
                    revision: revision.clone(),
                }),
                Some(now) if now != sealed => Some(SealViolation::ReleaseChangedSince {
                    release: sealed.version().clone(),
                    revision: revision.clone(),
                }),
                Some(_) => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        file_digest::FileDigest,
        git_revision::GitRevision,
        migration_file::{MigrationFile, MigrationFileError},
        migration_set::MigrationSet,
        release_tag::ReleaseTagError,
        release_version::{ReleaseVersion, ReleaseVersionError},
        seal_ledger::{
            SealLedger,
            SealLedgerError,
            sealed_release::{SealedMigration, SealedRelease},
        },
        seal_verification::{changed_since, release_violations, violations},
        seal_violation::SealViolation,
    };
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    /// Why a fixture could not be built.
    #[derive(Debug, thiserror::Error)]
    enum FixtureError {
        /// A fixture file name breaks the naming rule.
        #[error(transparent)]
        File(#[from] MigrationFileError),
        /// A fixture release does not parse.
        #[error(transparent)]
        Version(#[from] ReleaseVersionError),
        /// A fixture tag does not parse.
        #[error(transparent)]
        Tag(#[from] ReleaseTagError),
        /// The ledger could not be rendered.
        #[error(transparent)]
        Ledger(#[from] SealLedgerError),
    }

    fn file(name: &str) -> Result<MigrationFile, FixtureError> {
        Ok(MigrationFile::new(MigrationSet::Tenant, PathBuf::from("/root/migrations/tenant").join(name))?)
    }

    fn release(version: &str, files: &[&MigrationFile]) -> Result<SealedRelease, FixtureError> {
        Ok(SealedRelease::new(
            version.parse::<ReleaseVersion>()?,
            files.iter().map(|file| SealedMigration::new(file, FileDigest::of(b"select 1;\n"))).collect(),
        ))
    }

    #[test]
    fn a_ledger_written_by_the_tool_over_unchanged_files_holds() -> Result<(), FixtureError> {
        let entities = file("0001_entities.sql")?;
        let ledger = SealLedger::default().with_release(release("v1.0.0", &[&entities])?);
        let text = ledger.render()?;
        assert_eq!(violations(Some(&text), &ledger, &[(entities, b"select 1;\n".to_vec())])?, []);
        Ok(())
    }

    #[test]
    fn a_release_sealed_twice_an_empty_section_and_a_file_in_two_sections_are_each_named() -> Result<(), FixtureError> {
        let entities = file("0001_entities.sql")?;
        let ledger = SealLedger::default()
            .with_release(release("v1.0.0", &[&entities])?)
            .with_release(release("v1.0.0", &[])?)
            .with_release(release("v1.1.0", &[&entities])?);
        let text = ledger.render()?;
        assert_eq!(
            violations(Some(&text), &ledger, &[(entities.clone(), b"select 1;\n".to_vec())])?,
            [
                SealViolation::ReleaseSealedTwice("v1.0.0".parse()?),
                SealViolation::EmptyRelease("v1.0.0".parse()?),
                SealViolation::MigrationSealedTwice {
                    migration: SealedMigration::new(&entities, FileDigest::of(b"select 1;\n")),
                    first: "v1.0.0".parse()?,
                    second: "v1.1.0".parse()?,
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn a_stable_tag_needs_its_section_last_and_every_migration_sealed_and_a_pre_release_needs_neither() -> Result<(), FixtureError> {
        let entities = file("0001_entities.sql")?;
        let kinds = file("0002_kinds.sql")?;
        // clone: the tree holds the files the ledger and the expected violation also name
        let migrations = [(entities.clone(), b"select 1;\n".to_vec()), (kinds.clone(), b"select 1;\n".to_vec())];
        let first = SealLedger::default().with_release(release("v1.0.0", &[&entities])?);
        let both = first.clone().with_release(release("v1.1.0", &[&kinds])?);
        assert_eq!(release_violations(&both, &migrations, &"v1.1.0".parse()?), []);
        assert_eq!(release_violations(&first, &migrations, &"v1.1.0-rc.1".parse()?), []);
        assert_eq!(
            release_violations(&first, &migrations, &"v1.1.0".parse()?),
            [
                SealViolation::ReleaseNotSealed("v1.1.0".parse()?),
                SealViolation::UnsealedAtRelease {
                    file: kinds,
                    release: "v1.1.0".parse()?,
                },
            ]
        );
        assert_eq!(
            release_violations(&both, &migrations, &"v1.0.0".parse()?),
            [SealViolation::ReleaseNotLatest {
                release: "v1.0.0".parse()?,
                latest: "v1.1.0".parse()?,
            }]
        );
        Ok(())
    }

    #[test]
    fn only_the_sections_an_earlier_ledger_holds_are_compared() -> Result<(), FixtureError> {
        let entities = file("0001_entities.sql")?;
        let kinds = file("0002_kinds.sql")?;
        let revision = "v1.0.0".parse::<GitRevision>().unwrap_or_else(|never| match never {});
        let earlier = SealLedger::default().with_release(release("v1.0.0", &[&entities])?);
        let appended = earlier.clone().with_release(release("v1.1.0", &[&kinds])?);
        assert_eq!(changed_since(&appended, &earlier, &revision), []);
        let rewritten = SealLedger::default().with_release(release("v1.0.0", &[&entities, &kinds])?);
        assert_eq!(
            changed_since(&rewritten, &earlier, &revision),
            [SealViolation::ReleaseChangedSince {
                release: "v1.0.0".parse()?,
                revision,
            }]
        );
        Ok(())
    }
}
