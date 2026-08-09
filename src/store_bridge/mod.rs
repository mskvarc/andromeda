//! Trait implementation bridges between context-store repository structs
//! and binary-side service-layer traits.
//!
//! Each submodule implements one service-layer port trait on the concrete
//! repository struct that now lives in the context-store crate. The impls
//! stay here because the traits reference binary-side types (`AndromedaError`,
//! service-layer domain types) that the context-store cannot depend on.

mod entity;
mod outbox;
mod side_effects;
mod snapshot;
mod subscription;
