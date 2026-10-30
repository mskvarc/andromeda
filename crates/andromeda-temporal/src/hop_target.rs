//! One entity a split temporal query's hop reaches, as the hop judges it
//! (clauses 4.9 and 4.11, under `temporal_linked_retrieval`).
//!
//! A hop reads the linked entity's instances valid within the window of the
//! roots, and clause 4.11 counts among those the one each stream held when
//! the window opens, the latest across every holder of the entity. A target
//! is therefore read as the union of its instances inside the window and the
//! instances in effect at the start, either of which may be all it holds.

use crate::{answered_evolution::AnsweredEvolution, in_effect_entity::InEffectEntity};
use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::queried_entity::attribute_reading::QueriedEntity;

/// One hop target, as a hop reads it.
#[derive(Debug)]
pub(crate) struct HopTarget<'a> {
    /// The target's instances inside the window, every holder's merged;
    /// `None` when no holder has one there.
    pub(crate) window: Option<AnsweredEvolution<'a>>,
    /// The instances the target holds in effect when the window opens;
    /// `None` when it holds none or the window has no start.
    pub(crate) in_effect: Option<InEffectEntity<'a>>,
}

impl<'a> QueriedEntity<'a> for &'a HopTarget<'a> {
    type Instance = &'a InstanceDocument;

    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a InstanceDocument> {
        let inside = self.window.iter().flat_map(move |window| window.attribute_instances(attribute));
        inside.chain(self.in_effect.iter().flat_map(move |in_effect| in_effect.attribute_instances(attribute)))
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        let inside = self.window.iter().flat_map(QueriedEntity::types);
        inside.chain(self.in_effect.iter().flat_map(QueriedEntity::types))
    }
}
