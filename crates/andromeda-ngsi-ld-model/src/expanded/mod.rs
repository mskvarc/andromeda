//! Operations on expanded JSON-LD entities following NGSI-LD semantics (ETSI GS CIM 009 clause 4.6.2).
//!
//! Every function below these modules operates on the **expanded** form of
//! JSON-LD (full IRIs, array-wrapped values) as stored in the database. They
//! are pure functions with no I/O — the caller is responsible for loading and
//! persisting the entity.

pub mod attribute_change;
pub mod attribute_expiry;
pub mod attribute_system_time;
pub mod current_state_fold;
pub mod dataset_id;
pub mod entity_member;
pub mod geo_node;
pub mod geo_property;
pub mod geo_property_admission;
pub mod instance_key;
pub mod json_literal_merge;
pub mod merge;
pub mod mutation;
pub mod node;
pub mod null_sentinel;
pub mod partial_update_patch;
pub mod query_match;
pub mod removed_instance;
pub mod reserved_member;
pub mod scope_member;
pub mod unit_code_admission;
pub mod written_observation;
