//! The tooling around the SQL migrations, the schema contract of every release.
//!
//! The migrations are read as typed files (their set, version and name checked
//! against the layout every set shares) and brought to one canonical layout by
//! `pg_format`, with the fixups its options cannot express applied around it,
//! and linted by Squawk and by Andromeda's own rules over the schema the
//! migrations declare, every finding rendered as a diagnostic that quotes the
//! line it is on.

/// The canonical layout of one migration.
pub mod canonical_layout;
/// The schema the migrations declare.
pub mod declared_schema;
/// Where an external tool's executable is looked for.
pub mod executable_lookup;
/// The SHA-256 digest of a file's raw bytes.
pub mod file_digest;
/// How a migration's text ends.
pub mod file_ending;
/// `andromeda-sql format`.
pub mod format_command;
/// The words `pg_format` is taught, and the functions it knows.
pub mod formatter_vocabulary;
/// The function statements held out of `pg_format`.
pub mod function_statement;
/// A git revision, and one file as it holds it.
pub mod git_revision;
/// `andromeda-sql lint`.
pub mod lint_command;
/// One thing a linter found wrong with a migration.
pub mod lint_finding;
/// The rendering of a finding as a diagnostic.
pub mod lint_report;
/// A file a finding is in.
pub mod linted_source;
/// The rejoining of one-line parenthesised lists.
pub mod list_rejoin;
/// One migration file.
pub mod migration_file;
/// The name of a migration file and its naming rule.
pub mod migration_name;
/// The directories the migrations are kept in.
pub mod migration_set;
/// Every migration of the repository.
pub mod migration_tree;
/// The version a migration carries.
pub mod migration_version;
/// The `pg_format` executable and its configuration.
pub mod pg_format;
/// The PL/pgSQL syntax tree of a routine.
pub mod plpgsql_tree;
/// The tag a release is pushed under.
pub mod release_tag;
/// The stable release a seal is made for.
pub mod release_version;
/// Andromeda's own rules.
pub mod rule;
/// `andromeda-sql seal`.
pub mod seal_command;
/// The ledger of sealed migrations.
pub mod seal_ledger;
/// The checks that hold the migrations to the ledger.
pub mod seal_verification;
/// One way the seal is broken.
pub mod seal_violation;
/// A stretch of a migration's text.
pub mod source_range;
/// The top-level statements of an SQL text.
pub mod sql_statement;
/// The Squawk executable and its configuration.
pub mod squawk;
/// `andromeda-sql verify`.
pub mod verify_command;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-postgresql-context-store"],
    allow_workspace_dev_dependencies: ["andromeda-ngsi-ld-lexicon", "andromeda-postgresql-codes", "andromeda-postgresql-schema"],
    allow_production_dependencies: all,
}
