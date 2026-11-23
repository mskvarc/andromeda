//! Runtime configuration and advertisement for Andromeda's opt-in extensions.
//!
//! Andromeda ships a handful of capabilities the NGSI-LD specification does not
//! define. The rules they follow are:
//! runtime configuration rather than build features, off by default, reversible,
//! advertised under a vendor namespace with a token that carries an experimental
//! extension's revision, and never forwarded to a Context Source that has not
//! advertised support. This crate owns the machinery shared by every extension:
//! the configuration, the resolved enabled set, each extension's stability, the
//! advertisement, and the startup log of the conformance override and of the
//! extensions that are not stable. It exists so that the
//! HTTP layers that gate the features and the composition that wires them depend
//! on a neutral leaf rather than on each other. The one domain crate it names is the
//! query grammar, for the level of `q` target languages the enabled set serves
//! and a Context Source's advertisement announces.

pub mod advertisement;
pub mod enabled;
pub mod startup_log;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-json-literal",
        "andromeda-ngsi-ld-query",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
