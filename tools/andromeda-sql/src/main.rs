//! `andromeda-sql`: format, lint, seal and verify the SQL migrations.

use andromeda_sql::{
    canonical_layout::CanonicalLayout,
    executable_lookup::ExecutableLookup,
    format_command::{FormatError, FormatMode, FormatOutcome, format_migrations},
    git_revision::GitRevision,
    lint_command::{LintError, LintOutcome, lint_migrations},
    lint_finding::LintSeverity,
    lint_report::LintReport,
    pg_format::{self, PgFormat, PgFormatConfiguration, PgFormatError},
    release_tag::ReleaseTag,
    release_version::ReleaseVersion,
    seal_command::{SealError, SealOutcome, SealRefusal, seal_migrations},
    seal_violation::SealViolation,
    squawk::{self, Squawk, SquawkConfiguration, SquawkError},
    verify_command::{VerifyError, verify_migrations},
};
use clap::{Args, Parser, Subcommand};
use miette::{GraphicalReportHandler, GraphicalTheme};
use std::{
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::ExitCode,
};

/// The tooling around the Andromeda SQL migrations.
#[derive(Debug, Parser)]
#[command(name = "andromeda-sql", version)]
struct Cli {
    /// The repository root holding `migrations/` and the tool configuration
    /// files; the repository this tool is built from when not given.
    #[arg(long, global = true, value_name = "PATH")]
    root: Option<PathBuf>,
    /// What to do.
    #[command(subcommand)]
    command: Command,
}

/// The commands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Rewrite every migration that is not in its canonical layout.
    Format(FormatArgs),
    /// Lint every migration with Squawk and Andromeda's own rules, and print
    /// each finding where it is.
    Lint,
    /// Seal the migrations a stable release adds in the seal ledger.
    Seal(SealArgs),
    /// Check that no sealed migration or ledger section has changed.
    Verify(VerifyArgs),
}

/// The options of `seal`.
#[derive(Debug, Args)]
struct SealArgs {
    /// The stable release, as its tag `vX.Y.Z`.
    version: ReleaseVersion,
}

/// The options of `verify`.
#[derive(Debug, Args)]
struct VerifyArgs {
    /// Also fail unless every section the ledger holds at this git revision is
    /// unchanged in the working copy.
    #[arg(long, value_name = "REVISION")]
    against: Option<GitRevision>,
    /// Also fail unless the tree can be tagged as this release: for a stable
    /// `vX.Y.Z`, its section is the last one and every migration is sealed. A
    /// pre-release tag needs nothing more.
    #[arg(long, value_name = "TAG")]
    release: Option<ReleaseTag>,
}

/// The options of `format`.
#[derive(Debug, Args)]
struct FormatArgs {
    /// Report every migration that is not canonical and fail, writing nothing.
    #[arg(long)]
    check: bool,
}

/// Why a command could not run.
#[derive(Debug, thiserror::Error)]
enum CommandError {
    /// The working directory could not be read.
    #[error("the working directory could not be read: {0}")]
    WorkingDirectory(io::Error),
    /// `pg_format` could not be found or probed.
    #[error(transparent)]
    PgFormat(#[from] PgFormatError),
    /// The migrations could not be formatted.
    #[error(transparent)]
    Format(#[from] FormatError),
    /// Squawk could not be found or checked.
    #[error(transparent)]
    Squawk(#[from] SquawkError),
    /// The migrations could not be linted.
    #[error(transparent)]
    Lint(#[from] LintError),
    /// The migrations could not be sealed.
    #[error(transparent)]
    Seal(#[from] SealError),
    /// The seal could not be verified.
    #[error(transparent)]
    Verify(#[from] VerifyError),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")));
    let outcome = match cli.command {
        Command::Format(arguments) => {
            let mode = if arguments.check { FormatMode::Check } else { FormatMode::Write };
            run_format(&root, mode).map(|outcome| report_format(&outcome, mode))
        }
        Command::Lint => run_lint(&root).map(|outcome| report_lint(&outcome)),
        Command::Seal(arguments) => run_seal(&root, arguments.version).map(|outcome| report_seal(&outcome)),
        Command::Verify(arguments) => verify_migrations(&root, arguments.against.as_ref(), arguments.release.as_ref())
            .map_err(CommandError::from)
            .map(|violations| report_verify(&violations)),
    };
    outcome.unwrap_or_else(|error| {
        eprintln!("andromeda-sql: {error}");
        ExitCode::FAILURE
    })
}

/// Format the migrations below `root` in `mode`.
fn run_format(root: &Path, mode: FormatMode) -> Result<FormatOutcome, CommandError> {
    Ok(format_migrations(root, &CanonicalLayout::new(resolve_pg_format(root)?), mode)?)
}

/// Print what the run did and turn it into the exit status: a check that
/// found a migration not canonical fails.
fn report_format(outcome: &FormatOutcome, mode: FormatMode) -> ExitCode {
    match mode {
        FormatMode::Write => {
            for file in outcome.drifted() {
                println!("formatted {file}");
            }
            ExitCode::SUCCESS
        }
        FormatMode::Check => {
            for file in outcome.drifted() {
                eprintln!("not formatted: {file}");
            }
            match outcome.drifted().len() {
                0 => ExitCode::SUCCESS,
                drifted => {
                    eprintln!("andromeda-sql: {drifted} migration(s) are not formatted; run `cargo run -p andromeda-sql -- format` to fix");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// Lint the migrations below `root`.
fn run_lint(root: &Path) -> Result<LintOutcome, CommandError> {
    Ok(lint_migrations(root, &resolve_squawk(root)?, &resolve_pg_format(root)?)?)
}

/// Seal `release` over the migrations below `root`.
fn run_seal(root: &Path, release: ReleaseVersion) -> Result<SealOutcome, CommandError> {
    let layout = CanonicalLayout::new(resolve_pg_format(root)?);
    Ok(seal_migrations(root, release, &layout, &resolve_squawk(root)?)?)
}

/// The Squawk executable, configured for the repository at `root`.
fn resolve_squawk(root: &Path) -> Result<Squawk, CommandError> {
    let lookup = ExecutableLookup::from_environment(squawk::OVERRIDE_VARIABLE).map_err(CommandError::WorkingDirectory)?;
    Ok(Squawk::resolve(&lookup, SquawkConfiguration::at(root)?)?)
}

/// The `pg_format` executable, configured for the repository at `root`.
fn resolve_pg_format(root: &Path) -> Result<PgFormat, CommandError> {
    let lookup = ExecutableLookup::from_environment(pg_format::OVERRIDE_VARIABLE).map_err(CommandError::WorkingDirectory)?;
    Ok(PgFormat::resolve(&lookup, PgFormatConfiguration::at(root)?)?)
}

/// Print every finding where it is and turn the run into the exit status: any
/// finding fails.
fn report_lint(outcome: &LintOutcome) -> ExitCode {
    // A terminal follows the rule's documentation link; a log shows its address.
    let report = LintReport::new(GraphicalReportHandler::new_themed(GraphicalTheme::default()).with_links(io::stderr().is_terminal()));
    for linted in outcome.files() {
        for finding in linted.findings() {
            match report.render(linted.source(), linted.text(), finding) {
                Ok(rendered) => eprintln!("{rendered}"),
                Err(_) => eprintln!("{}: {}: {}", linted.source(), finding.rule(), finding.message()),
            }
        }
    }
    match (outcome.count(LintSeverity::Error), outcome.count(LintSeverity::Warning)) {
        (0, 0) => ExitCode::SUCCESS,
        (errors, warnings) => {
            eprintln!(
                "andromeda-sql: {errors} error(s) and {warnings} warning(s) in {} file(s)",
                outcome.files().len()
            );
            ExitCode::FAILURE
        }
    }
}

/// Print what the seal did and turn it into the exit status: a refused seal
/// fails.
fn report_seal(outcome: &SealOutcome) -> ExitCode {
    match outcome {
        SealOutcome::Sealed(release) => {
            for migration in release.migrations() {
                println!("sealed {migration}");
            }
            println!("sealed {} migration(s) for {}", release.migrations().len(), release.version());
            ExitCode::SUCCESS
        }
        SealOutcome::Refused(SealRefusal::Broken(violations)) => {
            report_verify(violations);
            eprintln!("andromeda-sql: the seal is broken, so nothing is sealed");
            ExitCode::FAILURE
        }
        SealOutcome::Refused(SealRefusal::NotAfterLatest { release, latest }) => {
            eprintln!("andromeda-sql: {release} does not follow {latest}, the last sealed release");
            ExitCode::FAILURE
        }
        SealOutcome::Refused(SealRefusal::NothingToSeal(release)) => {
            eprintln!("andromeda-sql: every migration is sealed already, so {release} has nothing to seal");
            ExitCode::FAILURE
        }
        SealOutcome::Refused(SealRefusal::NotFormatted(files)) => {
            for file in files {
                eprintln!("not formatted: {file}");
            }
            eprintln!("andromeda-sql: {} migration(s) are not formatted, so nothing is sealed", files.len());
            ExitCode::FAILURE
        }
        SealOutcome::Refused(SealRefusal::Lint(outcome)) => {
            report_lint(outcome);
            eprintln!("andromeda-sql: the lint fails, so nothing is sealed");
            ExitCode::FAILURE
        }
    }
}

/// Print every violation of the seal and turn them into the exit status: any
/// violation fails.
fn report_verify(violations: &[SealViolation]) -> ExitCode {
    for violation in violations {
        eprintln!("{violation}");
    }
    match violations.len() {
        0 => ExitCode::SUCCESS,
        broken => {
            eprintln!("andromeda-sql: {broken} violation(s) of the seal");
            ExitCode::FAILURE
        }
    }
}
