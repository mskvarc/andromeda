//! Programmable callback servers and protocol receivers for integration tests.

#![deny(missing_docs)]

pub mod callback_path;
pub mod callback_server;
pub mod context_source_server;
pub mod jsonld_context_server;
pub mod mqtt_receiver;
pub mod notification_server;
pub mod server_config;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-headers",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: all,
}
