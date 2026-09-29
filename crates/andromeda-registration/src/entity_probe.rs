//! The question registration policy asks of the local entity store before it
//! admits a proxied registration (clauses 5.9.2.4 and 5.9.3.4).
//!
//! An exclusive or redirect registration declares that the broker holds no
//! local data for the footprint it registers (clause 4.3.6.3), so creating one
//! over an entity the broker already stores is a `Conflict`. Deciding that needs
//! a look at the entity store, which registration policy must not depend on
//! directly; this port states exactly the two questions it asks and nothing
//! else about entities.

use crate::error::RegistrationError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_info::EntityInfo;
use futures::future::BoxFuture;
use iri_rs::IriBuf;

/// Whether the entity store holds something for a registration footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityPresence {
    /// At least one local entity matches the footprint.
    Present,
    /// No local entity matches the footprint.
    Absent,
}

/// Looks into the local entity store on behalf of registration policy.
///
/// Attribute names are the expanded IRIs of the registration's `propertyNames`
/// and `relationshipNames`, because registration `information` is expanded
/// before policy sees it (clause 5.5.7) and the store keys entities by IRI.
pub trait RegisteredEntityProbe: Send + Sync + 'static {
    /// Whether the entity `entity_id` exists locally and carries any of
    /// `attributes` (the exclusive-mode rule of clause 5.9.2.4).
    fn holds_any_attribute<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        entity_id: &'life1 Identifier,
        attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<EntityPresence, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Whether any local entity matches `selector` and, when `attributes` is
    /// non-empty, carries at least one of them (the redirect-mode rule of
    /// clause 5.9.2.4). A `None` selector constrains the entity axis not at all,
    /// which is how a registration that names attributes only reads.
    fn any_entity_matching<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        selector: Option<&'life1 EntityInfo>,
        attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<EntityPresence, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
