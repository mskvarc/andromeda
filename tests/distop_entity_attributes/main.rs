//! Distributed entity attributes operations integration tests.
//!
//! Tests for distributed entity attribute operations across context sources including:
//! - Append entity attributes (POST /entities/{entityId}/attrs)
//! - Update entity attributes (PATCH /entities/{entityId}/attrs)
//! - Partial update entity attribute (PATCH /entities/{entityId}/attrs/{attrId})
//! - Delete entity attribute (DELETE /entities/{entityId}/attrs/{attrId})
//!
//! Each operation is tested with different CSR registration modes:
//! - Inclusive: updates both local entity and forwards to context source
//! - Exclusive: forwards to context source for managed attributes, no local storage
//! - Redirect: forwards to one or more remote context sources, no local storage
//! - Auxiliary: local-only updates, no forwarding (retrieval mode only)
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `distop_entity_attrs_<operation>_<mode>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[path = "../support/context_source_adapter.rs"]
pub mod context_source_adapter;

mod append;
mod delete;
mod helpers;
mod partial_update;
mod update;
