//! The NGSI-LD write operations on an expanded entity payload (clause 5.6).

pub mod attribute;
pub mod deletion_sentinel;
pub mod entity;
pub mod error;
pub mod expiry;
pub(crate) mod scope_write;
pub mod selection;
#[cfg(test)]
pub(crate) mod speed_entity;
