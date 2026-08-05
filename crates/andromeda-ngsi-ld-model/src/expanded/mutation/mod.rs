//! The NGSI-LD write operations on an expanded entity payload (clause 5.6).

pub mod attribute;
pub mod entity;
pub mod error;
pub mod selection;
#[cfg(test)]
pub(crate) mod speed_entity;
