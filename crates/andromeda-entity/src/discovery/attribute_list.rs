//! Retrieve Available Attributes and their details (clauses 5.7.8 and 5.7.9).

use crate::{
    attribute_summary::AttributeSummary,
    discovery::{
        AttributeCollectionRepresentation,
        DiscoveryScope,
        EntityDiscoveryService,
        registry_union::extend_without_repetition,
        stored_names::parse_discovery_iri,
    },
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, ngsi_ld_urn::AttributeListId, uri::NGSI_LD_URN_NID};
use andromeda_ngsi_ld_model::attribute_list::{AttributeInfo, AttributeList, AttributeListType, AttributeResourceType};
use serde_json::Value;
use std::collections::HashMap;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// The entity types an attribute is known to belong to, across the local
/// store and the registry.
struct AttributeMetadata {
    entity_types: Vec<EntityTypeName>,
}

impl EntityDiscoveryService {
    /// List all known attributes, optionally including per-attribute detail.
    /// An attribute only the registry knows is reported with the entity types
    /// registered beside it, which may be none (clause 5.7.11).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when a summary page or the registry
    /// could not be read, [`EntityError::Identifier`] or
    /// [`EntityError::AttributeListIdentifier`] when the generated list
    /// identifier is not a well-formed NGSI-LD URN, and
    /// [`EntityError::Internal`] when a stored name does not read back as one.
    pub async fn attributes(
        &self,
        target: &ContextTarget,
        representation: AttributeCollectionRepresentation,
        scope: DiscoveryScope,
        context: &ContextSpec,
    ) -> Result<Value, EntityError> {
        // Keyset-paginate attribute summary aggregation.
        const PAGE: u32 = 500;
        let mut summary_rows: Vec<AttributeSummary> = Vec::new();
        let mut cursor: Option<AttributeName> = None;
        loop {
            let page = self.store.get_attribute_summary(target, cursor.as_ref(), PAGE).await?;
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|r| r.attr_iri.clone()); // clone: own cursor for next loop iteration
            let page_len = page.len();
            summary_rows.extend(page);
            if page_len < usize::try_from(PAGE).unwrap_or(usize::MAX) {
                break;
            }
        }

        // Build the same HashMap structure from SQL aggregation results
        let mut attr_data: HashMap<AttributeName, AttributeMetadata> = HashMap::with_capacity(summary_rows.len());
        for row in summary_rows {
            attr_data.insert(
                row.attr_iri,
                AttributeMetadata {
                    entity_types: row.entity_types,
                },
            );
        }
        for registered in self.registered_attributes(scope).await? {
            let metadata = attr_data
                .entry(registered.name)
                .or_insert_with(|| AttributeMetadata { entity_types: Vec::new() });
            extend_without_repetition(&mut metadata.entity_types, registered.entity_types);
        }

        match representation {
            AttributeCollectionRepresentation::Detailed => self.detailed_attributes(attr_data, context).await,
            AttributeCollectionRepresentation::List => self.attribute_name_list(attr_data.into_keys(), context).await,
        }
    }

    /// The clause 4.5.14 detailed attribute list: one record per attribute,
    /// carrying the full URI of the name, the short name the request's
    /// `@context` gives it, and the entity types it appears on.
    ///
    /// Table 5.2.28-1 restricts `attributeCount` and `attributeTypes` to
    /// cardinality 0..1 and clause 5.7.9.5 asks only for the names, so neither
    /// is reported here; clause 5.7.10 is where an attribute's families and
    /// count are answered.
    async fn detailed_attributes(&self, attr_data: HashMap<AttributeName, AttributeMetadata>, context: &ContextSpec) -> Result<Value, EntityError> {
        let mut attributes = Vec::with_capacity(attr_data.len());
        for (iri, meta) in attr_data {
            attributes.push(
                AttributeInfo::builder()
                    .attribute_name(self.compacted_attribute_name(&iri, context).await?)
                    .id(parse_discovery_iri(iri.to_string())?)
                    .type_(AttributeResourceType)
                    .type_names(self.compacted_type_names(meta.entity_types, context).await?)
                    .build(),
            );
        }
        attributes.sort_by(|left, right| left.id.cmp(&right.id));

        Ok(serde_json::to_value(attributes)?)
    }

    /// The clause 5.2.27 `AttributeList` of the attribute names, each
    /// compacted under `context`.
    async fn attribute_name_list(&self, attr_iris: impl IntoIterator<Item = AttributeName>, context: &ContextSpec) -> Result<Value, EntityError> {
        let mut compacted_names = self.compacted_attribute_names(attr_iris, context).await?;
        compacted_names.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let compacted_names = compacted_names;

        let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("AttributeList:{}", Uuid::new_v4())).build()?;
        let id = AttributeListId::try_from(id)?;

        let list = AttributeList {
            id,
            type_: AttributeListType,
            attribute_list: compacted_names,
        };

        Ok(serde_json::to_value(list)?)
    }
}
