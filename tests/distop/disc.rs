//! DISC area — discovery, Context Source Subscriptions, and the registration matching algorithm (ETSI GS CIM 009 v1.9.1, clauses 5.3.2, 5.3.3, 5.10, 5.11, 5.12).

/// A syntactically valid peer endpoint for registrations whose discovery and matching behaviour is under test on a single broker. Discovery and the matching algorithm are registry-local, so this endpoint is never contacted.
pub const PEER_ENDPOINT: &str = "http://broker-b:1056/ngsi-ld/v1";

mod csr_discovery;
mod csr_notification_delivery;
mod csr_notification_exactly_once;
mod csr_notification_periodic;
mod csr_notification_trigger;
mod csr_subscription;
mod csr_subscription_lifecycle;
mod discovery_projection;
mod discovery_query_entity;
mod discovery_query_geo_scope;
mod discovery_query_temporal;
mod discovery_query_validation;
mod discovery_retrieve;
mod matching_attribute;
mod matching_entity;
mod matching_loop_datasetid;
mod matching_temporal;
