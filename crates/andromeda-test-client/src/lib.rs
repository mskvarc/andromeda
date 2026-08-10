//! HTTP transport and NGSI-LD endpoint clients for integration tests.

#![deny(missing_docs)]

pub mod batch;
pub mod broker_endpoint;
pub mod context_source_registration;
pub mod discovery;
pub mod entity;
pub mod entity_attribute;
pub mod entity_map;
pub mod header;
pub mod jsonld_context;
pub mod problem_details;
pub mod raw_request;
pub mod response;
pub mod snapshot;
pub mod subscription;
pub mod temporal_entity;
pub mod tenant_name;
pub mod test_client;
pub mod transport_error;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-headers",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: all,
}
