//! NGSI-LD Data Integrity (ETSI GS CIM 019 v1.2.1) domain types.
//!
//! Provides strongly-typed representations of signed Atomic Entities,
//! Sealed Attributes, W3C Data Integrity proofs, and the transforms
//! that convert between them. This module contains types only; the
//! verification backend lives in [`crypto`] and the service wiring in
//! [`crate::service::provenance`]. Consumers import items by their
//! fully-qualified paths (no re-exports are provided here).

pub mod atomic_entity;
pub mod crypto;
pub mod cryptosuite;
pub mod error;
pub mod key;
pub mod ngsild_proof;
pub mod proof;
pub mod transform;
