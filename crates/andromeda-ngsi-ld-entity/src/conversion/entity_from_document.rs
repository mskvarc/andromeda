//! Turning a written entity into state: what Create Entity (clause 5.6.1),
//! Replace Entity (clause 5.6.18) and the canonical decoder do.
//!
//! The written document must name its entity and at least one type (clause
//! 4.5.1), carry no NGSI-LD Null (clause 5.2.1: a Null outside a partial
//! update or merge fragment is `BadRequestData`), and hold every attribute in a
//! shape a stored instance can take.

use crate::{
    attributes::Attributes,
    document::{EntityDocument, slot::Slot},
    entity_state::EntityState,
    entity_types::EntityTypes,
    refusal::EntityRefusal,
};

impl TryFrom<EntityDocument> for EntityState {
    type Error = EntityRefusal;

    fn try_from(document: EntityDocument) -> Result<EntityState, EntityRefusal> {
        let parts = document.into_parts();
        let id = parts.id.ok_or(EntityRefusal::MissingId)?;
        let types = EntityTypes::try_from_names(parts.types).map_err(|_| EntityRefusal::MissingType)?;
        let scope = match parts.scope {
            None => None,
            Some(Slot::Set(scope)) => Some(scope),
            Some(Slot::Remove) => return Err(EntityRefusal::ScopeNull),
        };
        let expires_at = match parts.expires_at {
            None => None,
            Some(Slot::Set(expires_at)) => Some(expires_at),
            Some(Slot::Remove) => return Err(EntityRefusal::NullInState { attribute: None }),
        };
        let attributes = Attributes::try_from_members(parts.attributes)?;
        Ok(EntityState::from_parts(id, types, scope, expires_at, attributes))
    }
}
