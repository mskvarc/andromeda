//! Typed failures raised while driving an integration-test stack.

use std::io;
use thiserror::Error;

/// Every way the integration-test runner can fail.
#[derive(Debug, Error)]
pub(crate) enum XtaskError {
    /// A named suite does not belong to any registered stack.
    #[error("unknown suite '{0}'")]
    UnknownSuite(String),
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
    /// Generating the HTTP/3 self-signed certificate failed.
    #[error("openssl failed to generate the HTTP/3 certificate")]
    CertGeneration,
    /// Building the blocking HTTP client for the health probe failed.
    #[error("failed to build the health-probe HTTP client: {0}")]
    HealthClient(#[from] reqwest::Error),
    /// Cloning the ETSI suite checkout failed.
    #[error("git clone of the ETSI test suite failed")]
    SuiteClone,
    /// Resetting the ETSI checkout to its pinned commit failed.
    #[error("git reset of the ETSI checkout to {0} failed")]
    SuiteReset(String),
    /// Fetching the pinned commit into an existing checkout failed.
    #[error("git fetch of the ETSI test suite failed")]
    SuiteFetch,
    /// `python3` is not on `PATH`, so the robot virtualenv cannot be built.
    #[error("python3 was not found on PATH; it is required to run the ETSI suite")]
    PythonMissing,
    /// Creating the robot virtualenv failed.
    #[error("creating the ETSI robot virtualenv failed")]
    VenvCreate,
    /// Installing the suite's Python requirements into the virtualenv failed.
    #[error("installing the ETSI suite's Python requirements failed")]
    PipInstall,
    /// A patch did not apply cleanly to the pristine checkout.
    #[error("patch did not apply cleanly to the ETSI checkout: {0}")]
    PatchApply(String),
    /// A requested suite subtree exists nowhere under the checkout.
    #[error("no such suite subtree under the ETSI checkout: {0}")]
    UnknownSubtree(String),
    /// The broker process exited while the runner was waiting for readiness.
    #[error("the broker exited during startup; see broker.log in the output directory")]
    BrokerStartupExit,
    /// A run's `output.xml` is absent, so a diff has nothing to read.
    #[error("robot output.xml not found at {0}")]
    OutputXmlMissing(String),
    /// A run's `output.xml` could not be parsed as XML.
    #[error("failed to parse {0}: {1}")]
    OutputXmlParse(String, String),
    /// A diff found test cases that pass in the baseline but fail in the candidate.
    #[error("{0} test case(s) newly failing since the baseline")]
    NewlyFailing(usize),
    /// An underlying process could not be spawned or waited on.
    #[error("process error: {0}")]
    Io(#[from] io::Error),
}
