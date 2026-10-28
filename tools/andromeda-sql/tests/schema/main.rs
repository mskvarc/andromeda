//! The schema the committed migrations declare, held to the shape the broker's
//! readers and writers rely on: the design pins of single relations, and the
//! byte bounds the migrations share with the broker's own constants.

mod attribute_history_index;
mod context_source_properties;
mod current_attributes;
mod dictionary_append_only;
mod entity_search_scope;
mod registration_information;
mod repository;
mod scope_history_index;
mod snapshot_branch;
mod storage_bound;
mod storage_parameter;
mod temporal_catalog;
mod temporal_intent_needs;
