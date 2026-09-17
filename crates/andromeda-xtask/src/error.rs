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
    /// An underlying process could not be spawned or waited on.
    #[error("process error: {0}")]
    Io(#[from] io::Error),
}
