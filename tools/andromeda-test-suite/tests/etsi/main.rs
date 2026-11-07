//! Conformance tests of ETSI GS CIM 009 against the base stack, grouped by the area of the specification they cover.
//!
//! Each test stores a vocabulary of its own and namespaces every identifier, so every test runs in the shared default
//! tenant beside every other.

mod batch_operations;
mod building_documents;
mod building_fixtures;
mod building_payloads;
mod common_behaviours;
mod context_information;
mod context_source;
mod distributed_operations;
mod entity_operations;
mod geometry_payloads;
mod jsonld_context;
mod linked_entity_fixtures;
mod linked_place_payloads;
mod mqtt_notification;
mod pagination_links;
mod registration_operations;
mod registration_payloads;
mod registration_subscription_operations;
mod subscription_operations;
mod subscription_payloads;
mod temporal_operations;
mod unchecked_requests;
mod vehicle_entity_payloads;
mod vehicle_evolution_payloads;
mod vehicle_fixtures;
