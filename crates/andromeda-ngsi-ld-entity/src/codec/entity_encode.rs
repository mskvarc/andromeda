//! Writing an entity node: `@id`, `@type`, `scope`, `expiresAt` and the
//! attributes.

use crate::{
    codec::{
        carrier_encode::{write_date_time, write_date_time_text},
        document_encode::write_members,
        instance_encode::write_attributes,
        value_encode::{NullValueOut, TextOut},
    },
    document::{EntityDocument, slot::Slot},
    entity_state::EntityState,
    scopes::Scopes,
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// One stored entity node.
pub(crate) struct StateNodeOut<'a>(pub(crate) &'a EntityState);

impl Serialize for StateNodeOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let state = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@id", state.id().as_str())?;
        map.serialize_entry("@type", &TypesOut(state.types().as_slice()))?;
        if let Some(scope) = state.scope() {
            map.serialize_entry(properties::SCOPE.as_str(), &ScopesOut(scope))?;
        }
        if let Some(expires_at) = state.expires_at() {
            write_date_time_text(&mut map, properties::EXPIRES_AT.as_str(), &expires_at.to_string())?;
        }
        write_attributes(&mut map, state.attributes())?;
        map.end()
    }
}

/// One written entity node.
pub(crate) struct DocumentNodeOut<'a>(pub(crate) &'a EntityDocument);

impl Serialize for DocumentNodeOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let document = self.0;
        let mut map = serializer.serialize_map(None)?;
        if let Some(id) = document.id() {
            map.serialize_entry("@id", id.as_str())?;
        }
        if !document.types().is_empty() {
            map.serialize_entry("@type", &TypesOut(document.types()))?;
        }
        match document.scope() {
            Some(Slot::Set(scope)) => map.serialize_entry(properties::SCOPE.as_str(), &ScopesOut(scope))?,
            Some(Slot::Remove) => map.serialize_entry(properties::SCOPE.as_str(), &[NullValueOut])?,
            None => {}
        }
        write_date_time(&mut map, properties::EXPIRES_AT.as_str(), document.expires_at())?;
        write_members(&mut map, document.attributes())?;
        map.end()
    }
}

/// The entity types.
struct TypesOut<'a>(&'a [EntityTypeName]);

impl Serialize for TypesOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(EntityTypeName::as_str))
    }
}

/// The scope paths, each as a value object.
struct ScopesOut<'a>(&'a Scopes);

impl Serialize for ScopesOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.as_slice().iter().map(|path| TextOut(path.as_str())))
    }
}
