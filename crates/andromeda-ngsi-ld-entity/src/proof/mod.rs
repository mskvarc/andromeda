//! The `ngsildproof` of a sealed attribute (ETSI GS CIM 009 v1.9.1 clause
//! 4.5.2.2, ETSI TS 104 179 V1.1.1 clause 5.2.3), typed.
//!
//! A seal is admitted only as a sub-attribute of the attribute it signs:
//! Table 5.2.5-1 and its siblings make `ngsildproof` a member of the eight
//! attribute families and of no entity, and TS 104 179 clause 3.1 defines a
//! sealed attribute as an attribute carrying one. The broker stores and serves
//! the seal; it neither signs nor verifies.

pub mod attribute_proof;
pub mod cryptosuite;
pub mod data_integrity_proof;
pub mod proof_fault;
pub mod proof_member;
pub mod proof_seals;
pub mod proof_value;
#[cfg(test)]
mod seal_write_tests;
pub mod written_seals;
