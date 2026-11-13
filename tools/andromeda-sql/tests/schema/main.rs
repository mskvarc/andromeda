//! The schema the committed migrations declare, held to the shape the broker's
//! readers and writers rely on: the design pins of single relations, the byte
//! bounds the migrations share with the broker's own constants, and the
//! compact codes the last sealed release shipped.

mod attribute_history_index;
mod claim_columns;
mod compact_code_freeze;
mod context_source_properties;
mod current_attributes;
mod dictionary_append_only;
mod entity_search_scope;
mod evolution_horizon_index;
mod geometry_reach_statistics;
mod jsonld_contexts_index;
mod notification_events_index;
mod posted_queries;
mod registration_information;
mod replica_identity;
mod repository;
mod scope_history_index;
mod snapshot_branch;
mod storage_bound;
mod storage_parameter;
mod temporal_catalog;
mod temporal_intent_needs;
mod trigram_collation;
