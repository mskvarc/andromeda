//! Narrowing the merged instances of every entity a Context Source
//! contributed to a split temporal query (clause 5.7.4.4).
//!
//! The last step of clause 5.7.4.4 includes only the instances "which meet the
//! temporal, query and geoquery restrictions", as [`InstanceRestriction`]
//! states the rule. Under `splitEntities` the restrictions are decided once
//! the parts are aggregated, so the store keeps every row of a contributed
//! entity and its merged instances are narrowed here. Each instance of a
//! narrowed Attribute is read on its own by the leaves that read that
//! Attribute, and every other leaf holds as it held over the entity's
//! gathered parts when the settlement judged them.
//!
//! An entity the narrowing leaves no instance is no part of the answer unless
//! the query names a `datasetId`, as the local read leaves out an entity it
//! reads no instance of (clause 5.7.4.4 step S8 removes Attributes, never the
//! Entity).

use crate::{
    hop_targets::{HopReading, HopTargets},
    read::dataset_step::DatasetStep,
    reduced_instance::ReducedInstance,
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::{
    split_filter::LeafTruths,
    temporal::{compiled::CompiledTemporalQuery, contributed_entities::ContributedEntities, instance_restriction::InstanceRestriction},
};
use andromeda_ngsi_ld_query_evaluation::split_filter::leaf_truths;
use std::collections::HashMap;

/// What narrowing the instances of the contributed entities a split query
/// kept reads: the query, the entities its hops reach, and the truth of every
/// leaf over each entity's gathered parts.
#[derive(Debug)]
pub struct SplitNarrowing {
    /// The query whose `q` and geo-query narrow the instances, boxed so a
    /// settlement stays small beside the verdicts that carry it.
    compiled: Box<CompiledTemporalQuery>,
    /// The entities the Query Terms' hops reach, read over the window.
    hops: HopReading,
    /// The truth of every leaf over the gathered parts of each contributed
    /// entity.
    truths: HashMap<Identifier, LeafTruths>,
}

impl SplitNarrowing {
    /// The narrowing `compiled` makes, its hops reaching what `hops` read,
    /// over entities whose gathered parts hold the leaves as `truths` says.
    pub(crate) fn new(compiled: CompiledTemporalQuery, hops: HopReading, truths: HashMap<Identifier, LeafTruths>) -> SplitNarrowing {
        SplitNarrowing {
            compiled: Box::new(compiled),
            hops,
            truths,
        }
    }

    /// Keep, of every entity of `entities` a source contributed to, the
    /// instances that meet the query and geoquery restrictions, and leave out
    /// one the narrowing leaves no instance when no `datasetId` keeps it. An
    /// entity no source contributed to was narrowed by the store and is left
    /// as it is, and so is an aggregated one, which is rendered from its
    /// narrowed instances.
    pub fn narrow(&self, entities: &mut Vec<TemporalHistory>, contributed: &ContributedEntities) {
        let restriction = InstanceRestriction::of(&self.compiled.entities);
        let attributes = restriction.narrowed_attributes();
        if attributes.is_empty() {
            return;
        }
        let narrowing = DocumentNarrowing {
            restriction,
            attributes: &attributes,
            linked: HopTargets::of(&self.hops, self.compiled.window),
            narrowing: self,
        };
        let dataset_step = DatasetStep::of(&self.compiled);
        entities.retain_mut(|history| match history {
            TemporalHistory::Instances(document) if document.id().is_some_and(|entity_id| contributed.contains(entity_id)) => {
                narrowing.narrow(document);
                dataset_step == DatasetStep::Present || document.instance_count() > 0
            }
            TemporalHistory::Instances(_) | TemporalHistory::Aggregates(_) => true,
        });
    }
}

/// One narrowing of the contributed documents, set up once for all of them.
struct DocumentNarrowing<'n> {
    /// The rule the instances are kept by.
    restriction: InstanceRestriction<'n>,
    /// The Attributes the rule narrows.
    attributes: &'n [&'n AttributeIri],
    /// The entities the hops reach.
    linked: HopTargets<'n>,
    /// The query and the truths over the gathered parts.
    narrowing: &'n SplitNarrowing,
}

impl DocumentNarrowing<'_> {
    /// Keep the instances of `document` that meet the restriction.
    fn narrow(&self, document: &mut TemporalDocument) {
        let filter = self.restriction.filter();
        let none = LeafTruths::none(filter.leaf_count());
        let entity = document.id().and_then(|entity_id| self.narrowing.truths.get(entity_id)).unwrap_or(&none);
        // clone: the types are read while the document's instances are narrowed in place
        let types: Vec<EntityTypeName> = document.types().to_vec();
        let window = self.narrowing.compiled.window;
        document.narrow_instances(|name, instance| {
            let Some(attribute) = self.attributes.iter().copied().find(|attribute| attribute.as_str() == name.as_str()) else {
                return true;
            };
            let reduced = ReducedInstance {
                attribute,
                instance,
                types: &types,
                window,
            };
            self.restriction.keeps(attribute, entity, &leaf_truths(&filter, &reduced, &self.linked))
        });
    }
}

#[cfg(test)]
mod tests;
