//! What the query languages read of an entity state or a written document.
//!
//! The `q` language (clause 4.9), the geo-query (clause 4.10) and the scope
//! query (clause 4.19) are evaluated by `andromeda-ngsi-ld-query` against the
//! entity traits it defines. This module implements them for
//! [`EntityState`](crate::entity_state::EntityState), the state a store holds
//! and a notification carries, and for
//! [`EntityDocument`](crate::document::EntityDocument), the instances a
//! Temporal Evolution write records. Every read borrows from the model; nothing
//! is rebuilt as JSON to be evaluated. [`linked_states`] holds the states a
//! `q` hop crosses into.

pub mod document_reading;
pub mod family_values;
pub mod held_instance;
pub mod linked_states;
pub mod queried_mode;
pub mod state_reading;
pub mod value_node_reading;
pub mod written_instance;

#[cfg(test)]
mod document_reading_tests;
#[cfg(test)]
mod state_reading_tests;
