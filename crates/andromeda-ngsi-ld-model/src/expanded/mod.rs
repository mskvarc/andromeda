//! Operations on expanded JSON-LD entities following NGSI-LD semantics (ETSI GS CIM 009 clause 4.6.2).
//!
//! Every function below these modules operates on the **expanded** form of
//! JSON-LD (full IRIs, array-wrapped values) as stored in the database. They
//! are pure functions with no I/O — the caller is responsible for loading and
//! persisting the entity.

pub mod dataset_id;
pub mod entity_member;
pub mod geo_property;
pub mod instance_key;
pub mod merge;
pub mod mutation;
pub mod node;
pub mod null_sentinel;
pub mod query_match;
pub mod removed_instance;
