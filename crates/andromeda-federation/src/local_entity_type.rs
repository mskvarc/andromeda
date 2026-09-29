//! Resolving a locally-held entity's type for federated-write coverage.
//!
//! A single-entity provision operation that carries no `type` in its request --
//! an attribute-level write such as `DELETE /entities/{id}/attrs/{attr}` or
//! `POST /entities/{id}/attrs` -- still has to be matched against registrations
//! by the entity's actual type. Without it,
//! [`crate::registration_selection::information_matches_entity`] treats an absent
//! type as satisfying every selector's type axis (`entity_type.is_none_or(..)`),
//! so a registration scoped to an unrelated type is wrongly taken to cover the
//! write and, being proxied and not advertising the operation, refuses it with a
//! 409 (clause 5.9.2.4). This port lets the write path read the local entity's
//! expanded type so coverage is decided by the type the entity actually has. An
//! entity this broker does not hold locally resolves to `None`, and matching then
//! falls back to the entity identifier exactly as before.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use futures::future::BoxFuture;
use iri_rs::IriBuf;

/// Resolves the expanded types of an entity this broker holds locally.
pub trait LocalEntityTypeResolver: Send + Sync {
    /// Every expanded type IRI the locally-stored entity `id` under `target`
    /// carries -- clause 5.12 matches a registration against any of an entity's
    /// types, so all are returned. Empty when the entity is not held locally: its
    /// types cannot then constrain coverage, so matching falls back to the id.
    fn local_entity_types<'resolver>(&'resolver self, target: &'resolver ContextTarget, id: &'resolver Identifier) -> BoxFuture<'resolver, Vec<IriBuf>>;
}
