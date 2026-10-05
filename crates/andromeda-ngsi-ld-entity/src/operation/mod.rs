//! The writes clause 5.6 defines, applied to an entity's state.
//!
//! Every write takes the written document by reference and copies only what
//! it stores. An entity's attributes are shared ([`Attributes`]), so a write
//! copies the attributes it touches and nothing else. A write that fails
//! leaves the state partly written: callers write into a clone of the
//! previous state, which is cheap, and drop it on failure.
//!
//! [`Attributes`]: crate::attributes::Attributes

pub mod append;
pub mod attribute_write;
pub mod delete_attribute;
pub mod expiry_removal;
pub mod family_write;
pub mod instance_patch;
pub mod merge;
pub mod mutation_error;
pub mod partial_update;
pub mod purge;
pub mod replace_attribute;
pub mod scope_write;
pub mod selection;
#[cfg(test)]
pub(crate) mod speed_entity;
pub mod sub_attribute_patch;
pub mod update;
pub mod value_merge;
