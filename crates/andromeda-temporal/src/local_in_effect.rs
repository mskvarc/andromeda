//! What this broker holds in effect of each entity a Context Source
//! contributed to a split temporal query, when its window opens (clauses 4.11
//! and 5.7.4.4).
//!
//! The local part is one of the parts whose in-effect instances are merged
//! before the filters judge them, so it is read into the same document a
//! source's answer is, every instance carrying the system times a window on
//! `createdAt` or `modifiedAt` places it by.

use crate::{error::TemporalError, port::read_scope::ScopeReach, service::TemporalService, temporal_invariant::TemporalInvariant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity_system_times::EntitySystemTimes,
    temporal::{TemporalHistoryRow, temporal_document::TemporalDocument},
};
use andromeda_ngsi_ld_query::split_filter::SplitFilter;
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
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the query does not compile, the context
    /// does not resolve, the instances cannot be read, or a stored row no
    /// longer reads as the instance it recorded.
    pub async fn read_local_in_effect(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        context: &ContextSpec,
        entity_ids: &[Identifier],
    ) -> Result<LocalInEffect, TemporalError> {
        let compiled = self.compile_temporal_query(params, context).await?;
        if entity_ids.is_empty() || SplitFilter::of(&compiled.entities).is_empty() || compiled.window.in_effect_start().is_none() {
            return Ok(LocalInEffect::default());
        }
        let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
        let rows = self.reader.in_effect_instances(&scope, &compiled, entity_ids).await?;
        let parts = rows
            .into_iter()
            .map(|(entity_id, rows)| part_document(&entity_id, rows).map(|part| (entity_id, part)))
            .collect::<Result<_, _>>()?;
        Ok(LocalInEffect { parts })
    }
}

/// The document the `rows` this broker holds of `entity_id` make up, every
/// instance carrying its system times, as a split query judges a part.
///
/// # Errors
///
/// Returns [`TemporalInvariant::RenderedInstance`] when a stored row no longer
/// reads as the instance it recorded.
pub(crate) fn part_document(entity_id: &Identifier, rows: Vec<TemporalHistoryRow>) -> Result<TemporalDocument, TemporalError> {
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
    let entity = EntityDocument::new(Some(entity_id.clone()), Vec::new(), None, None, attributes); // clone: the document owns the identifier it names
    Ok(TemporalDocument::new(entity, EntitySystemTimes::default(), IndexMap::new()))
}
