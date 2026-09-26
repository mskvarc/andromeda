//! A single-entity write payload the clause 5.5.4 admission produced.
//!
//! The admission expands a payload exactly the way its write path stores it
//! and screens the expanded form. What it expanded is kept rather than thrown
//! away, so the local write consumes it directly and a payload is expanded
//! once per request. Only a payload the forwarding phase narrowed -- some of
//! its attributes claimed by an exclusive or redirect registration (clause
//! 4.3.6.3) -- is admitted again, because what remains of it is a different
//! document.

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
use serde_json::Value;
use std::marker::PhantomData;

/// An entity-shaped payload admitted for the write `W`, expanded as that
/// write stores it.
///
/// Only [`EntityService::admit_entity_write`](crate::EntityService::admit_entity_write)
/// and [`EntityService::admit_remaining_entity`](crate::EntityService::admit_remaining_entity)
/// construct one, so holding one proves the payload passed clause 5.5.4.
#[derive(Debug)]
pub struct AdmittedEntity<W> {
    /// The expanded, screened payload.
    expanded: Value,
    /// The write the payload was admitted for.
    write: PhantomData<W>,
}

impl<W> AdmittedEntity<W> {
    /// Wrap a payload the admission of `W` expanded and screened.
    pub(crate) const fn new(expanded: Value) -> AdmittedEntity<W> {
        AdmittedEntity { expanded, write: PhantomData }
    }

    /// The expanded payload the write stores.
    pub(crate) fn into_expanded(self) -> Value {
        self.expanded
    }
}

/// A bare Attribute Fragment admitted for the write `W` on `attribute`,
/// expanded as that write stores it.
///
/// The fragment is expanded under the attribute the request URI names (see
/// `andromeda_jsonld::attribute_fragment`), so the attribute travels with it.
#[derive(Debug)]
pub struct AdmittedAttribute<'a, W> {
    /// The attribute the fragment was expanded under.
    attribute: &'a AttributeName,
    /// The expanded, screened fragment.
    expanded: Value,
    /// The write the fragment was admitted for.
    write: PhantomData<W>,
}

impl<'a, W> AdmittedAttribute<'a, W> {
    /// Wrap a fragment the admission of `W` expanded under `attribute`.
    pub(crate) const fn new(attribute: &'a AttributeName, expanded: Value) -> AdmittedAttribute<'a, W> {
        AdmittedAttribute {
            attribute,
            expanded,
            write: PhantomData,
        }
    }

    /// The attribute the fragment was expanded under.
    pub(crate) const fn attribute(&self) -> &'a AttributeName {
        self.attribute
    }

    /// The expanded fragment the write stores.
    pub(crate) fn into_expanded(self) -> Value {
        self.expanded
    }
}

/// A compact write payload, in the JSON form the admission expands.
pub trait WritePayload: Send {
    /// Render the payload as the compact JSON document the admission expands.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when a decoded payload cannot be
    /// rendered back to JSON.
    fn into_payload(self) -> Result<Value, EntityError>;
}

impl WritePayload for Value {
    fn into_payload(self) -> Result<Value, EntityError> {
        Ok(self)
    }
}

impl WritePayload for &Entity {
    fn into_payload(self) -> Result<Value, EntityError> {
        serde_json::to_value(self).map_err(|error| EntityError::Validation(EntityValidationReason::EntityPayload(error)))
    }
}

impl WritePayload for &EntityPatch {
    fn into_payload(self) -> Result<Value, EntityError> {
        serde_json::to_value(self).map_err(|error| EntityError::Validation(EntityValidationReason::PatchPayload(error)))
    }
}

#[cfg(test)]
mod tests {
    use crate::admitted_write::WritePayload;
    use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
    use serde_json::{Value, json};

    #[test]
    fn a_json_document_is_its_own_payload() {
        let document = json!({"speed": {"type": "Property", "value": 1}});
        let expected = document.to_string();
        pretty_assertions::assert_eq!(document.into_payload().expect("a document").to_string(), expected);
    }

    #[test]
    fn a_decoded_entity_and_patch_render_back_to_their_compact_json() {
        let written = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 1}});
        let entity: Entity = serde_json::from_value(written.clone()).expect("an entity"); // clone: the test compares against the written document
        pretty_assertions::assert_eq!((&entity).into_payload().expect("renders"), serde_json::to_value(&entity).expect("renders"));
        let patch: EntityPatch = serde_json::from_value(written).expect("a patch");
        let rendered: Value = (&patch).into_payload().expect("renders");
        pretty_assertions::assert_eq!(rendered, serde_json::to_value(&patch).expect("renders"));
    }
}
