//! A single-entity write document the clause 5.5.4 admission produced.
//!
//! The admission expands a payload exactly the way its write path stores it
//! and screens the written document. What it expanded is kept rather than
//! thrown away, so the local write consumes it directly and a payload is
//! expanded once per request. The forwarding phase reads the same document, and
//! what it leaves to the local phase, the document without the attributes an
//! exclusive or redirect registration claimed (clause 4.3.6.3), is a part of
//! what was admitted, so it is never admitted again.

use crate::compact_geometry_admission::admit_compact_geometries;
use andromeda_ngsi_ld_entity::document::{EntityDocument, instance_document::InstanceDocument};
use andromeda_ngsi_ld_expansion::{
    attribute_fragment::AttributeFragment,
    entity_expansion::EntityExpansion,
    error::ExpansionError,
    normalization::Normalization,
    written_member::WrittenMember,
};
use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    attribute_strip::AttributeStrip,
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;
use std::marker::PhantomData;

/// An entity-shaped document admitted for the write `W`, expanded as that
/// write stores it.
///
/// Only [`EntityService::admit_entity_write`](crate::EntityService::admit_entity_write)
/// constructs one, so holding one proves the payload passed clause 5.5.4.
#[derive(Debug)]
pub struct AdmittedEntity<W> {
    /// The expanded, screened document.
    document: EntityDocument,
    /// The write the document was admitted for.
    write: PhantomData<W>,
}

impl<W> AdmittedEntity<W> {
    /// Wrap a document the admission of `W` expanded and screened.
    pub(crate) const fn new(document: EntityDocument) -> AdmittedEntity<W> {
        AdmittedEntity { document, write: PhantomData }
    }

    /// The admitted document, which the forwarding phase hands to the
    /// registrations the write matches (clauses 5.6.1.4 and following).
    #[must_use]
    pub const fn document(&self) -> &EntityDocument {
        &self.document
    }

    /// What the local phase of a distributed write writes once the attributes
    /// proxied registrations own (clause 4.3.6.3) are taken out of the
    /// admitted document: "any remaining input data" of clauses 5.6.1.4 and
    /// following.
    ///
    /// The proxied attributes are named by the expanded names the forwarding
    /// phase reported. What remains of an admitted document is part of it, so
    /// it needs no admission of its own: a document left with no attribute
    /// leaves the local phase nothing to write, which is `None`.
    #[must_use]
    pub fn local_remainder(mut self, proxied: &[AttributeName]) -> Option<AdmittedEntity<W>> {
        let before = self.document.attributes().len();
        self.document.attributes_mut().retain(|name, _| !proxied.contains(name));
        match AttributeStrip::from_counts(before, self.document.attributes().len()) {
            AttributeStrip::Emptied => None,
            AttributeStrip::Untouched | AttributeStrip::Narrowed => Some(self),
        }
    }

    /// Give up the written document the write applies.
    pub(crate) fn into_document(self) -> EntityDocument {
        self.document
    }
}

/// A bare Attribute Fragment admitted for the write `W` on `attribute`,
/// expanded as that write stores it.
///
/// The fragment is expanded under the attribute the request URI names, so the
/// attribute's expanded name travels with it.
#[derive(Debug)]
pub struct AdmittedAttribute<'a, W> {
    /// The attribute as the request URI names it.
    attribute: &'a AttributeName,
    /// The attribute's expanded name, under which the fragment was expanded.
    expanded_name: AttributeName,
    /// The expanded, screened instance.
    document: InstanceDocument,
    /// The write the fragment was admitted for.
    write: PhantomData<W>,
}

impl<'a, W> AdmittedAttribute<'a, W> {
    /// Wrap a fragment the admission of `W` expanded under `attribute`.
    pub(crate) fn new(attribute: &'a AttributeName, fragment: AttributeFragment) -> AdmittedAttribute<'a, W> {
        let (expanded_name, document) = fragment.into_parts();
        AdmittedAttribute {
            attribute,
            expanded_name,
            document,
            write: PhantomData,
        }
    }

    /// The attribute as the request URI names it.
    pub(crate) const fn attribute(&self) -> &'a AttributeName {
        self.attribute
    }

    /// The expanded name of the attribute the fragment writes.
    #[must_use]
    pub const fn expanded_name(&self) -> &AttributeName {
        &self.expanded_name
    }

    /// The admitted instance, which the forwarding phase hands to the
    /// registrations the write matches.
    #[must_use]
    pub const fn instance(&self) -> &InstanceDocument {
        &self.document
    }

    /// Give up the expanded attribute name and the written instance.
    pub(crate) fn into_parts(self) -> (AttributeName, InstanceDocument) {
        (self.expanded_name, self.document)
    }
}

/// A write payload as the transport decoded it into the typed compact model,
/// which the admission expands.
pub trait WritePayload: Send {
    /// What the payload holds under an attribute name.
    type Member;

    /// Refuse a payload whose typed geometries are past `limit` before it is
    /// expanded.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadBudgetExceeded::GeometryPositions`] naming the first
    /// attribute whose geometry is past `limit`.
    fn admit_geometries(&self, limit: GeometryPositionLimit) -> Result<(), PayloadBudgetExceeded>;

    /// Expand the payload under `expansion`'s context, after the concise
    /// rewrite `normalization` names.
    fn expand<'s>(
        self,
        expansion: &'s EntityExpansion<'s>,
        normalization: Normalization,
    ) -> impl Future<Output = Result<EntityDocument, ExpansionError>> + Send + 's
    where
        Self: 's;
}

impl<M: WrittenMember + Sync> WritePayload for &Entity<M> {
    type Member = M;

    fn admit_geometries(&self, limit: GeometryPositionLimit) -> Result<(), PayloadBudgetExceeded> {
        admit_compact_geometries(limit, self.attribute_members())
    }

    fn expand<'s>(
        self,
        expansion: &'s EntityExpansion<'s>,
        normalization: Normalization,
    ) -> impl Future<Output = Result<EntityDocument, ExpansionError>> + Send + 's
    where
        Self: 's,
    {
        expansion.entity(self, normalization)
    }
}

impl<M: WrittenMember + Sync> WritePayload for &EntityPatch<M> {
    type Member = M;

    fn admit_geometries(&self, limit: GeometryPositionLimit) -> Result<(), PayloadBudgetExceeded> {
        admit_compact_geometries(limit, self.attribute_members())
    }

    fn expand<'s>(
        self,
        expansion: &'s EntityExpansion<'s>,
        normalization: Normalization,
    ) -> impl Future<Output = Result<EntityDocument, ExpansionError>> + Send + 's
    where
        Self: 's,
    {
        expansion.entity_patch(self, normalization)
    }
}

#[cfg(test)]
mod tests {
    use crate::admitted_write::WritePayload;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
    use andromeda_ngsi_ld_model::{
        attribute::merge_member::MergeMember,
        entity::{Entity, EntityPatch},
    };
    use serde_json::json;

    /// A decoded entity, a decoded patch and a decoded merge fragment of one
    /// compact document expand to the same written document.
    #[tokio::test]
    async fn every_payload_form_expands_to_the_same_document() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds");
        let expansion = EntityExpansion::prepare(&engine, &ContextSpec::core_only())
            .await
            .expect("the core context processes");
        let written = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 1}, "owner": "urn:ngsi-ld:Person:1"});
        let entity: Entity = serde_json::from_value(written.clone()).expect("an entity");
        let patch: EntityPatch = serde_json::from_value(written.clone()).expect("a patch");
        let fragment: EntityPatch<MergeMember> = serde_json::from_value(written).expect("a merge fragment");
        let from_entity = (&entity).expand(&expansion, Normalization::Entity).await.expect("the entity expands");
        let from_patch = (&patch).expand(&expansion, Normalization::Entity).await.expect("the patch expands");
        let from_fragment = (&fragment).expand(&expansion, Normalization::Entity).await.expect("the fragment expands");
        pretty_assertions::assert_eq!(from_patch, from_entity);
        pretty_assertions::assert_eq!(from_fragment, from_entity);
    }
}
