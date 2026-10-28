//! The fixtures the rule tests build their schemas from: migrations written
//! inline, read into a schema, and one rule run over them.

use crate::{
    declared_schema::{DeclaredSchema, DeclaredSchemaError},
    file_digest::FileDigest,
    formatter_vocabulary::{FormatterVocabulary, VocabularyFile, VocabularyList},
    linted_source::SourceKey,
    migration_file::{MigrationFile, MigrationFileError},
    migration_set::MigrationSet,
    release_version::{ReleaseVersion, ReleaseVersionError},
    rule::{Rule, RuleContext},
    seal_ledger::{
        SealLedger,
        sealed_release::{SealedMigration, SealedRelease},
    },
};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

/// Why a fixture could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    /// A fixture path names no migration set directory.
    #[error("{0} names no migration set")]
    Set(String),
    /// A fixture file name breaks the naming rule.
    #[error(transparent)]
    File(#[from] MigrationFileError),
    /// A fixture migration could not be read.
    #[error(transparent)]
    Schema(#[from] DeclaredSchemaError),
    /// The fixture release does not parse.
    #[error(transparent)]
    Release(#[from] ReleaseVersionError),
}

/// The release every fixture seal is made for.
const FIXTURE_RELEASE: &str = "v1.0.0";

/// The schema `migrations`, each `<set>/<file>` with its text, declare.
///
/// # Errors
///
/// Returns [`FixtureError`] when a fixture is not a readable migration.
pub fn schema(migrations: &[(&str, &str)]) -> Result<DeclaredSchema, FixtureError> {
    let files = migrations
        .iter()
        .map(|(path, text)| Ok((fixture_file(path)?, Arc::<str>::from(*text))))
        .collect::<Result<Vec<_>, FixtureError>>()?;
    Ok(DeclaredSchema::read(files)?)
}

/// The migration file of the fixture path `<set>/<file>`.
fn fixture_file(path: &str) -> Result<MigrationFile, FixtureError> {
    let set = path
        .split_once('/')
        .and_then(|(set, _)| set.parse::<MigrationSet>().ok())
        .ok_or_else(|| FixtureError::Set(path.to_owned()))?;
    Ok(MigrationFile::new(set, PathBuf::from("/root/migrations").join(path))?)
}

/// A vocabulary of no words in which `pg_format` knows `known`.
#[must_use]
pub fn vocabulary(functions: &str, keywords: &str, known: &[&str]) -> FormatterVocabulary {
    FormatterVocabulary::new(
        VocabularyList::new(VocabularyFile::Functions, Arc::from(functions)),
        VocabularyList::new(VocabularyFile::Keywords, Arc::from(keywords)),
        known.iter().map(|name| (*name).to_owned()).collect::<BTreeSet<_>>(),
    )
}

/// Where `rule` finds something in `migrations`, each place written as
/// `<file>:<line>:<column>`, one-based.
///
/// # Errors
///
/// Returns [`FixtureError`] when a fixture is not a readable migration.
pub fn findings(rule: Rule, migrations: &[(&str, &str)]) -> Result<Vec<String>, FixtureError> {
    findings_with(rule, migrations, &vocabulary("", "", &[]))
}

/// Where `rule` finds something in `migrations` read with `vocabulary`.
///
/// # Errors
///
/// Returns [`FixtureError`] when a fixture is not a readable migration.
pub fn findings_with(rule: Rule, migrations: &[(&str, &str)], vocabulary: &FormatterVocabulary) -> Result<Vec<String>, FixtureError> {
    findings_in(rule, migrations, vocabulary, &SealLedger::default())
}

/// Where `rule` finds something in `migrations` once one release has sealed
/// the fixtures named in `sealed`.
///
/// # Errors
///
/// Returns [`FixtureError`] when a fixture is not a readable migration.
pub fn findings_sealing(rule: Rule, migrations: &[(&str, &str)], sealed: &[&str]) -> Result<Vec<String>, FixtureError> {
    let sealed = migrations
        .iter()
        .filter(|(path, _)| sealed.contains(path))
        .map(|(path, text)| Ok(SealedMigration::new(&fixture_file(path)?, FileDigest::of(text.as_bytes()))))
        .collect::<Result<Vec<_>, FixtureError>>()?;
    let ledger = SealLedger::default().with_release(SealedRelease::new(FIXTURE_RELEASE.parse::<ReleaseVersion>()?, sealed));
    findings_in(rule, migrations, &vocabulary("", "", &[]), &ledger)
}

/// Where `rule` finds something in `migrations` read with `vocabulary` and
/// `ledger`.
fn findings_in(rule: Rule, migrations: &[(&str, &str)], vocabulary: &FormatterVocabulary, ledger: &SealLedger) -> Result<Vec<String>, FixtureError> {
    let schema = schema(migrations)?;
    let context = RuleContext::new(&schema, vocabulary, ledger);
    Ok(rule
        .check(&context)
        .iter()
        .map(|found| {
            let file = match found.source() {
                SourceKey::Migration(version) => schema.migration(version).map(|migration| migration.file().to_string()).unwrap_or_default(),
                SourceKey::Vocabulary(file) => file.to_string(),
            };
            format!("{file}:{}", found.finding().range().start())
        })
        .collect())
}
