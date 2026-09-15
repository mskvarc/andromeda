//! Runtime configuration and advertisement for Andromeda's opt-in extensions.
//!
//! Andromeda ships a handful of capabilities the NGSI-LD specification does not
//! define. `docs/studies/extension-strategy.md` sets the rules they follow:
//! runtime configuration rather than build features, off by default, reversible,
//! advertised under a vendor namespace, and never forwarded to a Context Source
//! that has not advertised support. This crate owns the machinery shared by
//! every extension — the configuration, the resolved enabled set, the
//! advertisement, and the conformance-override startup log — so that the HTTP
//! layers that gate the features and the composition that wires them depend on a
//! neutral leaf rather than on each other.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod advertisement;
pub mod config;
pub mod enabled;
pub mod startup_log;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-stack",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
