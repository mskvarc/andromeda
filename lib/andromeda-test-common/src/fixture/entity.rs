//! Typed wrappers over entity fixtures in `fixtures/entity/`.
//!
//! Batches 2–5 populate this module with factories such as
//! `building_minimal(&scope)` that load the matching JSON file and apply
//! scope-derived placeholder substitutions. During Batch 0 the module is
//! deliberately empty — migrating a suite is what first adds a factory,
//! and authoring them up-front without a real consumer would just grow
//! dead code.

#![allow(clippy::missing_const_for_fn)]
