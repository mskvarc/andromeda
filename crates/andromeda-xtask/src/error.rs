//! Typed failures raised while driving an integration-test stack.

use crate::{etsi::suite::PINNED_COMMIT, stack::Suite};
use std::{io, path::PathBuf};
use thiserror::Error;

/// Every way the integration-test runner can fail.
#[derive(Debug, Error)]
pub(crate) enum XtaskError {
    /// The selected suites are owned by more than one stack, which would need more than one topology.
    #[error("the selected suites span multiple stacks; run one stack's suites per invocation")]
    SuitesSpanMultipleStacks,
    /// A `--suite` selection resolved to no suites at all.
    #[error("no suites were selected")]
    NoSuitesSelected,
    /// `--local-broker` was combined with a stack other than base.
    #[error("--local-broker is only valid for the base stack")]
    LocalBrokerRequiresBase,
    /// A host port required by `--local-broker` is already bound.
    #[error("port {0} is already in use; stop the process holding it first")]
    PortInUse(u16),
    /// A host broker never answered its health probe.
    #[error("the broker on port {0} did not become ready")]
    BrokerNotReady(u16),
    /// `docker compose up` reported a non-zero exit status.
    #[error("docker compose up failed")]
    ComposeUp,
    /// `cargo build` of the broker binary reported a non-zero exit status.
    #[error("cargo build of the broker failed")]
    CargoBuild,
    /// `cargo test` reported a non-zero exit status.
    #[error("cargo test failed")]
    CargoTest,
    /// `cargo test --no-run` failed to compile the integration-test suites.
    #[error("cargo test --no-run failed to compile the suites")]
    TestCompile,
    /// A line of cargo's `--message-format=json` stream could not be parsed.
    #[error("failed to parse a cargo build message: {0}")]
    BuildMessageParse(#[source] serde_json::Error),
    /// The prebuilt-test manifest could not be read from disk.
    #[error("failed to read the prebuilt test manifest {path}: {source}")]
    TestManifestRead {
        /// Where the manifest was expected.
        path: PathBuf,
        /// Why reading it failed.
        #[source]
        source: io::Error,
    },
    /// The prebuilt-test manifest on disk is not valid manifest JSON.
    #[error("failed to parse the prebuilt test manifest {path}: {source}")]
    TestManifestParse {
        /// Where the manifest was read from.
        path: PathBuf,
        /// Why its contents were rejected.
        #[source]
        source: serde_json::Error,
    },
    /// The prebuilt-test manifest could not be serialized for writing.
    #[error("failed to serialize the prebuilt test manifest: {0}")]
    TestManifestSerialize(#[source] serde_json::Error),
    /// The `url -> file` map pinning the suite's `@context` documents could not
    /// be serialized.
    #[error("failed to serialize the suite context preload map: {0}")]
    ContextPreloadSerialize(#[source] serde_json::Error),
    /// A registered suite has no compiled binary, either in the manifest or in
    /// the build that was meant to produce one.
    #[error("no compiled binary for suite '{}'", .0.name)]
    SuiteBinaryMissing(&'static Suite),
    /// Generating a stack's test certificate material failed.
    #[error("openssl failed to generate the stack's test certificates")]
    CertGeneration,
    /// Building the blocking HTTP client for the health probe failed.
    #[error("failed to build the health-probe HTTP client: {0}")]
    HealthClient(#[from] reqwest::Error),
    /// Cloning the ETSI suite checkout failed.
    #[error("git clone of the test suite failed")]
    SuiteClone,
    /// Resetting the ETSI checkout to its pinned commit failed.
    #[error("git reset of the suite checkout to {commit} failed", commit = PINNED_COMMIT)]
    SuiteReset,
    /// Fetching the pinned commit into an existing checkout failed.
    #[error("git fetch of the test suite failed")]
    SuiteFetch,
    /// `python3` is not on `PATH`, so the robot virtualenv cannot be built.
    #[error("python3 was not found on PATH; it is required to run the test suite")]
    PythonMissing,
    /// Creating the robot virtualenv failed.
    #[error("creating the robot virtualenv failed")]
    VenvCreate,
    /// Installing the suite's Python requirements into the virtualenv failed.
    #[error("installing the suite's Python requirements failed")]
    PipInstall,
    /// A patch did not apply cleanly to the pristine checkout.
    #[error("patch did not apply cleanly to the suite checkout: {0}")]
    PatchApply(PathBuf),
    /// A requested suite subtree exists nowhere under the checkout.
    #[error("no such suite subtree under the checkout: {0}")]
    UnknownSubtree(PathBuf),
    /// The broker process exited while the runner was waiting for readiness.
    #[error("the broker exited during startup; see broker.log in the output directory")]
    BrokerStartupExit,
    /// `rebot` could not be spawned to merge the per-pass reports into one.
    #[error("failed to spawn rebot to merge the reports: {0}")]
    RebotSpawn(#[source] io::Error),
    /// `rebot` exited non-zero while merging the per-pass reports.
    #[error("rebot failed to merge the per-pass reports")]
    RebotFailed,
    /// A run's `output.xml` is absent, so a diff has nothing to read.
    #[error("robot output.xml not found at {0}")]
    OutputXmlMissing(PathBuf),
    /// A run's `output.xml` could not be parsed as XML.
    #[error("failed to parse {path}: {source}")]
    OutputXmlParse {
        /// The `output.xml` that was read.
        path: PathBuf,
        /// Why its text is not well-formed XML.
        #[source]
        source: roxmltree::Error,
    },
    /// A diff found test cases that pass in the baseline but fail in the candidate.
    #[error("{0} test case(s) newly failing since the baseline")]
    NewlyFailing(usize),
    /// A `--strict` run recorded failing test cases, so the run fails as a gate.
    #[error("{0} conformance test case(s) failed")]
    EtsiTestsFailed(usize),
    /// `pg_format` is not installed or not on `PATH`.
    #[error("pg_format was not found on PATH; install it with 'brew install pgformatter'")]
    PgFormatMissing,
    /// `pg_format` exited non-zero while formatting a migration.
    #[error("pg_format failed to format {0}")]
    PgFormatFailed(PathBuf),
    /// `pg_format` emitted output that is not valid UTF-8.
    #[error("pg_format produced non-UTF-8 output for {0}")]
    PgFormatOutputNotUtf8(PathBuf),
    /// `pg_format` dropped, duplicated or reordered the placeholder of a
    /// function statement, so the statement cannot be put back.
    #[error("pg_format did not keep the function statement placeholders of {0}")]
    PgFormatLostFunction(PathBuf),
    /// A SQL formatting fixup pattern failed to compile.
    #[error("failed to compile a SQL formatting fixup pattern: {0}")]
    SqlFixupPattern(#[source] regex::Error),
    /// The migrations directory could not be walked.
    #[error("failed to walk the migrations directory: {0}")]
    SqlDiscover(#[source] walkdir::Error),
    /// A migration could not be read for formatting.
    #[error("failed to read the migration {path}: {source}")]
    SqlRead {
        /// The migration that could not be read.
        path: PathBuf,
        /// Why reading it failed.
        #[source]
        source: io::Error,
    },
    /// A reformatted migration could not be written back.
    #[error("failed to write the migration {path}: {source}")]
    SqlWrite {
        /// The migration that could not be written.
        path: PathBuf,
        /// Why writing it failed.
        #[source]
        source: io::Error,
    },
    /// One or more migrations are not in canonical format under `--check`.
    #[error("{0} migration(s) are not formatted; run 'cargo xtask format-sql' to fix")]
    SqlFormatDrift(usize),
    /// An underlying process could not be spawned or waited on.
    #[error("process error: {0}")]
    Io(#[from] io::Error),
}
