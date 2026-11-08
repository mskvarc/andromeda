//! What this broker holds in effect of each entity a Context Source
//! contributed to a split temporal query, when its window opens (clauses 4.11
//! and 5.7.4.4).
//!
//! The local part is one of the parts whose in-effect instances are merged
//! before the filters judge them, so it is read into the same document a
//! source's answer is, every instance carrying the system times a window on
//! `createdAt` or `modifiedAt` places it by.

use crate::{
    error::TemporalError,
    port::{
        in_effect_reads::{InEffectReads, ScopeRead},
        read_scope::ScopeReach,
    },
    query_reading::QueryReading,
    service::TemporalService,
    temporal_invariant::TemporalInvariant,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity_system_times::EntitySystemTimes,
    temporal::{TemporalHistoryRow, temporal_document::TemporalDocument},
};
use andromeda_ngsi_ld_query::{projection::attrs_selection::AttrsAdmission, split_filter::SplitFilter};
use andromeda_ngsi_ld_render::temporal::history_row::neutral_row_to_instance;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use indexmap::IndexMap;
use std::collections::HashMap;

/// The part this broker holds in effect of each contributed entity when a
/// split query's window opens.
#[derive(Debug, Default)]
pub struct LocalInEffect {
    /// The in-effect instances of each entity holding any, by entity.
    parts: HashMap<Identifier, TemporalDocument>,
}

impl LocalInEffect {
    /// The in-effect part of `entity_id`, if this broker holds one.
    #[must_use]
    pub fn part_of(&self, entity_id: &Identifier) -> Option<&TemporalDocument> {
        self.parts.get(entity_id)
    }

    /// Every in-effect part this broker holds.
    pub fn parts(&self) -> impl Iterator<Item = &TemporalDocument> {
        self.parts.values()
    }
}

impl TemporalService {
    /// Read what this broker holds in effect of each of `entity_ids` when the
    /// window of `params` opens: the instances the query's filters read, and
    /// the Scope in force. Empty when the query carries no filter or its
    /// window has no instance in effect before it.
    ///
    /// The request's terms and `q` target languages are read as `reading`
    /// says.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the query does not compile, the context
    /// does not resolve, the instances cannot be read, or a stored row no
    /// longer reads as the instance it recorded.
    pub async fn read_local_in_effect(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        reading: QueryReading<'_>,
        entity_ids: &[Identifier],
    ) -> Result<LocalInEffect, TemporalError> {
        let compiled = self
            .compile_temporal_query(params, reading.context, reading.language_targeting, AttrsAdmission::ReifiedScopeAdmitted)
            .await?;
        let filter = SplitFilter::of(&compiled.entities);
        if entity_ids.is_empty() || filter.is_empty() || compiled.window.in_effect_start().is_none() {
            return Ok(LocalInEffect::default());
        }
        let attributes = filter.read_attributes();
        let reads = InEffectReads {
            attributes: &attributes,
            scope: if filter.reads_scope() { ScopeRead::Read } else { ScopeRead::Unread },
        };
        let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
        let rows = self.reader.in_effect_instances(&scope, &compiled, entity_ids, reads).await?;
        let parts = rows
            .into_iter()
            .map(|(entity_id, rows)| part_document(&entity_id, &[], rows).map(|part| (entity_id, part)))
            .collect::<Result<_, _>>()?;
        Ok(LocalInEffect { parts })
    }
}

/// The document the `rows` this broker holds of `entity_id` make up, every
/// instance carrying its system times, as a split query judges a part, the
/// entity named by `types`, which a part whose types another part names may
/// leave empty.
///
/// # Errors
///
/// Returns [`TemporalInvariant::RenderedInstance`] when a stored row no longer
/// reads as the instance it recorded.
pub(crate) fn part_document(entity_id: &Identifier, types: &[EntityTypeName], rows: Vec<TemporalHistoryRow>) -> Result<TemporalDocument, TemporalError> {
    let mut instances: IndexMap<AttributeName, Vec<InstanceDocument>> = IndexMap::new();
    for row in rows {
        // clone: the document keys each attribute by the name its rows carry
        let attribute = row.attribute_name.clone();
        let instance =
            neutral_row_to_instance(row, SysAttrsInclusion::Include).map_err(|error| TemporalError::from(TemporalInvariant::RenderedInstance(error)))?;
        instances.entry(attribute).or_default().push(instance);
    }
    let attributes = instances
        .into_iter()
        .map(|(attribute, instances)| (attribute, AttributeMember::Instances(instances)))
        .collect();
    let entity = EntityDocument::new(Some(entity_id.clone()), types.to_vec(), None, None, attributes); // clone: the document owns the identifier and the types it names
    Ok(TemporalDocument::new(entity, EntitySystemTimes::default(), IndexMap::new()))
}

#[cfg(test)]
mod tests {
    use crate::local_in_effect::part_document;
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        identifier::Identifier,
        instance_identity::InstanceIdentity,
        time::date_time::NgsiLdDateTime,
        time_prop::TimeProp,
    };
    use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;

    /// A window on `createdAt` or `modifiedAt` places the local part's
    /// instances by their system times, so the document keeps them, and a
    /// deletion keeps the instant it was deleted at.
    #[test]
    fn the_part_keeps_the_system_times_and_the_deletion_instant() {
        let at = |raw: &str| raw.parse::<NgsiLdDateTime>().expect("a valid instant").get();
        let entity: Identifier = "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier");
        let speed = |value: TemporalValue, observed: Option<&str>, id: &str| TemporalHistoryRow {
            attribute_name: "https://example.com/speed".parse::<AttributeName>().expect("a valid attribute IRI"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value,
            observed_at: observed.map(at),
            recorded_at: at("2020-08-01T11:00:00Z"),
            created_at: Some(at("2020-08-01T10:00:00Z")),
            modified_at: Some(at("2020-08-01T11:00:00Z")),
            instance: EvolutionInstance::Attribute(InstanceIdentity::Verbatim(id.parse().expect("a valid URI"))),
            metadata: None,
            stream_extent: StreamExtent::Whole,
        };
        let rows = vec![
            speed(
                TemporalValue::Number {
                    number: JsonNumber::try_from(60.0).expect("a finite number"),
                    datatype: None,
                },
                Some("2020-08-01T10:30:00Z"),
                "urn:ngsi-ld:Instance:1",
            ),
            speed(
                TemporalValue::Tombstone {
                    deleted_at: at("2020-08-01T11:30:00Z"),
                },
                None,
                "urn:ngsi-ld:Instance:2",
            ),
        ];
        let part = part_document(&entity, &[], rows).expect("the rows read as instances");
        let instances = part.entity().attributes().get("https://example.com/speed").expect("speed").instances();
        pretty_assertions::assert_eq!(instances.len(), 2);
        let value = instances.first().expect("the value");
        pretty_assertions::assert_eq!(value.instant_on(TimeProp::Modified), Some(at("2020-08-01T11:00:00Z")));
        pretty_assertions::assert_eq!(value.instant_on(TimeProp::Created), Some(at("2020-08-01T10:00:00Z")));
        let deletion = instances.get(1).expect("the deletion");
        pretty_assertions::assert_eq!(deletion.instant_on(TimeProp::Deleted), Some(at("2020-08-01T11:30:00Z")));
        pretty_assertions::assert_eq!(part.id(), Some(&entity));
    }

    /// The split-query placement reads a deletion's own `observedAt`, so the
    /// part keeps it on a removed Scope; only the response drops it.
    #[test]
    fn the_part_keeps_the_observed_at_of_a_removed_scope() {
        let at = |raw: &str| raw.parse::<NgsiLdDateTime>().expect("a valid instant").get();
        let entity: Identifier = "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier");
        let scope = expanded_properties::SCOPE.as_str();
        let row = TemporalHistoryRow {
            attribute_name: scope.parse::<AttributeName>().expect("a valid attribute IRI"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value: TemporalValue::Tombstone {
                deleted_at: at("2020-08-01T11:30:00Z"),
            },
            observed_at: Some(at("2020-08-01T11:00:00Z")),
            recorded_at: at("2020-08-01T11:30:00Z"),
            created_at: Some(at("2020-08-01T10:00:00Z")),
            modified_at: Some(at("2020-08-01T11:30:00Z")),
            instance: EvolutionInstance::Attribute(InstanceIdentity::Verbatim("urn:ngsi-ld:Instance:1".parse().expect("a valid URI"))),
            metadata: None,
            stream_extent: StreamExtent::Whole,
        };
        let part = part_document(&entity, &[], vec![row]).expect("the row reads as an instance");
        let removed = part.entity().attributes().get(scope).expect("scope").instances();
        pretty_assertions::assert_eq!(
            removed.first().expect("the removal").instant_on(TimeProp::Observed),
            Some(at("2020-08-01T11:00:00Z"))
        );
    }
}
