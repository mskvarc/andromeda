//! `andromeda-sql lint`: run every linter over the migrations and collect what
//! each one finds, file by file.
//!
//! Squawk runs first. Andromeda's own rules then run over the schema the
//! migrations declare, unless Squawk found a migration it could not read at
//! all: that run already fails, and the schema cannot be read either.
//!
//! A sealed migration is part of the schema the rules read, but nothing is
//! reported in it: it can no longer change, so a finding there could never be
//! put right and would block every later seal.

use crate::{
    declared_schema::{DeclaredSchema, DeclaredSchemaError},
    formatter_vocabulary::{FormatterVocabulary, VocabularyError, VocabularyFile},
    lint_finding::{LintFinding, LintSeverity},
    linted_source::{LintedSource, SourceKey},
    migration_file::MigrationFile,
    migration_tree::{MigrationTree, MigrationTreeError},
    migration_version::MigrationVersion,
    pg_format::PgFormat,
    rule::{Rule, RuleContext},
    seal_ledger::{SealLedger, SealLedgerError},
    squawk::{Squawk, SquawkError},
};
use andromeda_postgresql_context_store::schema_contract::server_requirements::REQUIRED_POSTGRES_VERSION;
use std::{
    collections::BTreeMap,
    fs,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// One linted file together with its text and what the linters found in it.
#[derive(Debug)]
pub struct LintedFile {
    /// The file.
    source: LintedSource,
    /// Its text, shared by every diagnostic that quotes it.
    text: Arc<str>,
    /// What the linters found, in the order of their position in the file.
    findings: Vec<LintFinding>,
}

impl LintedFile {
    /// The file.
    #[must_use]
    pub const fn source(&self) -> &LintedSource {
        &self.source
    }

    /// Its text.
    #[must_use]
    pub const fn text(&self) -> &Arc<str> {
        &self.text
    }

    /// What the linters found, in the order of their position in the file.
    #[must_use]
    pub fn findings(&self) -> &[LintFinding] {
        &self.findings
    }
}

/// What a lint run found over every file.
#[derive(Debug)]
pub struct LintOutcome {
    /// Every file with at least one finding, migrations first in version order.
    files: Vec<LintedFile>,
}

impl LintOutcome {
    /// Every file with at least one finding, migrations first in version order.
    #[must_use]
    pub fn files(&self) -> &[LintedFile] {
        &self.files
    }

    /// How many findings of `severity` there are over every file.
    #[must_use]
    pub fn count(&self, severity: LintSeverity) -> usize {
        self.files
            .iter()
            .flat_map(|file| &file.findings)
            .filter(|finding| finding.severity() == severity)
            .count()
    }
}

/// Lint every migration below `root/migrations` with Squawk, against the
/// oldest PostgreSQL release the broker supports, and with Andromeda's own
/// rules, asking `pg_format` which functions it knows. Only the migrations the
/// seal ledger does not hold are reported on.
///
/// # Errors
///
/// Returns [`LintError`] when the migrations, the ledger or the vocabulary
/// cannot be read, a linter fails, or a linter reports a file it was not
/// given.
pub fn lint_migrations(root: &Path, squawk: &Squawk, pg_format: &PgFormat) -> Result<LintOutcome, LintError> {
    let ledger = SealLedger::load(root)?;
    let mut migrations = Vec::new();
    for file in MigrationTree::read(root)?.into_files() {
        match fs::read_to_string(file.path()) {
            Ok(text) => migrations.push((file, Arc::<str>::from(text))),
            Err(source) => return Err(LintError::Read { file: Box::new(file), source }),
        }
    }
    let mut files = BTreeMap::new();
    let mut keys = BTreeMap::new();
    for (file, text) in migrations.iter().filter(|(file, _)| !ledger.seals(file)) {
        keys.insert(file.repository_path(), SourceKey::Migration(file.version()));
        files.insert(
            SourceKey::Migration(file.version()),
            LintedFile {
                // clone: the linted file and the schema both keep the migration
                source: LintedSource::Migration(file.clone()),
                text: Arc::clone(text),
                findings: Vec::new(),
            },
        );
    }
    let squawk_findings = if keys.is_empty() {
        Vec::new()
    } else {
        squawk.lint(root, keys.keys(), REQUIRED_POSTGRES_VERSION)?
    };
    let unreadable = squawk_findings.iter().any(|(_, finding)| finding.severity() == LintSeverity::Error);
    for (path, finding) in squawk_findings {
        let key = keys.get(&path).ok_or(LintError::UnknownFile(path))?;
        if let Some(file) = files.get_mut(key) {
            file.findings.push(finding);
        }
    }
    if !unreadable {
        lint_schema(root, pg_format, &ledger, migrations, &mut files)?;
    }
    let mut files = files.into_values().filter(|file| !file.findings.is_empty()).collect::<Vec<_>>();
    for file in &mut files {
        file.findings.sort_by_key(|finding| finding.range().start());
    }
    Ok(LintOutcome { files })
}

/// Run Andromeda's own rules over the schema `migrations` declare, adding
/// each finding to its file in `files` and passing over the ones in a
/// migration `ledger` seals.
fn lint_schema(
    root: &Path,
    pg_format: &PgFormat,
    ledger: &SealLedger,
    migrations: Vec<(MigrationFile, Arc<str>)>,
    files: &mut BTreeMap<SourceKey, LintedFile>,
) -> Result<(), LintError> {
    let schema = DeclaredSchema::read(migrations)?;
    let vocabulary = FormatterVocabulary::probe(root, pg_format, schema.calls().iter().map(|call| call.name().name()))?;
    for found in Rule::check_all(&RuleContext::new(&schema, &vocabulary, ledger)) {
        let key = found.source();
        let file = match key {
            SourceKey::Migration(version) => match files.get_mut(&key) {
                Some(file) => file,
                None if ledger.seals_version(version) => continue,
                None => return Err(LintError::UnknownMigration(version)),
            },
            SourceKey::Vocabulary(file) => files.entry(key).or_insert_with(|| vocabulary_file(&vocabulary, file)),
        };
        file.findings.push(found.into_finding());
    }
    Ok(())
}

/// The linted file of the vocabulary file `file`.
fn vocabulary_file(vocabulary: &FormatterVocabulary, file: VocabularyFile) -> LintedFile {
    let list = match file {
        VocabularyFile::Functions => vocabulary.functions(),
        VocabularyFile::Keywords => vocabulary.keywords(),
    };
    LintedFile {
        source: LintedSource::Vocabulary(file),
        text: Arc::clone(list.text()),
        findings: Vec::new(),
    }
}

/// Why the migrations could not be linted.
#[derive(Debug, thiserror::Error)]
pub enum LintError {
    /// The migrations could not be read.
    #[error(transparent)]
    Tree(#[from] MigrationTreeError),
    /// A migration could not be read.
    #[error("{file} could not be read: {source}")]
    Read {
        /// The migration concerned.
        file: Box<MigrationFile>,
        /// What reading it failed with.
        source: io::Error,
    },
    /// Squawk could not lint the migrations.
    #[error(transparent)]
    Squawk(#[from] SquawkError),
    /// A linter reported a file it was not given.
    #[error("a linter reported {}, which it was not given", .0.display())]
    UnknownFile(PathBuf),
    /// A rule reported a migration the schema was not read from.
    #[error("a rule reported migration {0}, which it was not given")]
    UnknownMigration(MigrationVersion),
    /// The schema the migrations declare could not be read.
    #[error(transparent)]
    Schema(#[from] DeclaredSchemaError),
    /// The formatter's vocabulary could not be read or probed.
    #[error(transparent)]
    Vocabulary(#[from] VocabularyError),
    /// The seal ledger could not be read.
    #[error(transparent)]
    Ledger(#[from] SealLedgerError),
}
