//! The filters a drawn case asks: a `q` with the `jsonKeys` and `expandValues` it is asked with, whether it reads
//! through a Relationship, and a geoquery it may carry beside it.
//!
//! A filter is drawn from one Entity of its case, its anchor, in the state after its creation or after its change.
//! Its `q` is one to four terms joined by `;` and `|` and grouped up to two deep, most of them drawn from the anchor
//! and the others free. When the anchor holds a `GeoProperty` in that state, the filter now and then also carries a
//! geoquery applied to it, its reference geometry the Attribute's own or one of the pool.

use crate::{
    geo_generation::{
        drawn_reference::drawn_reference,
        drawn_relation::{DrawnRelation, drawn_relation, geo_query},
        pooled_geometry::PooledGeometry,
    },
    q_generation::{
        anchored_term::anchored_term,
        drawn_entity::{DrawnEntity, DrawnState},
        drawn_pool::NAMES,
        drawn_query::{DrawnQuery, union},
        free_term::free_term,
        placeholder,
    },
};
use andromeda_q_corpus::corpus::Traversal;
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityId, EntityTypeName},
    query::geoquery::GeoQuery,
};
use proptest::{
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
    strategy::Union,
};
use std::sync::Arc;

/// The weight of a term drawn from the anchor.
const ANCHORED: u32 = 5;

/// The weight of a free term.
const FREE: u32 = 1;

/// The geoquery a drawn filter carries beside its `q`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
#[display("{relation:?} {geometry:?} geoproperty={geoproperty}")]
pub struct DrawnGeoPart {
    /// Its relation.
    pub relation: DrawnRelation,
    /// Its reference geometry.
    pub geometry: PooledGeometry,
    /// The `GeoProperty` it is applied to, which `geoproperty` names.
    pub geoproperty: &'static str,
}

/// One drawn filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnFilter {
    /// The `q` as drawn, writing the placeholders of the case for its identifiers and its type.
    pub q: String,
    /// The Attributes `jsonKeys` names, in the order of [`NAMES`].
    pub json_keys: Vec<&'static str>,
    /// The Attributes `expandValues` names, in the order of [`NAMES`].
    pub expand_values: Vec<&'static str>,
    /// Whether the `q` reads through a Relationship, which Query Entities admits with `join` and `joinLevel`.
    pub traversal: Traversal,
    /// The geoquery asked beside the `q`, when the filter carries one.
    pub geo: Option<DrawnGeoPart>,
}

impl DrawnFilter {
    /// The `q` as a client writes it, `ids` identifying the Entity at each position and `entity_type` the type of the
    /// case.
    ///
    /// # Errors
    ///
    /// Fails when a placeholder names a position `ids` has no identifier at.
    pub fn resolved_q(&self, ids: &[EntityId], entity_type: &EntityTypeName) -> Result<String> {
        placeholder::resolved(&self.q, ids, entity_type)
    }

    /// The geoquery asked beside the `q`, None when the filter carries none.
    ///
    /// # Errors
    ///
    /// Fails when the geoquery does not build.
    pub fn geo_query(&self) -> Result<Option<GeoQuery>> {
        self.geo.map(|part| geo_query(part.relation, part.geometry, Some(part.geoproperty))).transpose()
    }
}

/// A `q` of up to four terms grouped up to two deep, drawn from the Entity at `position` of `entities` in `state`,
/// joined by `|` more often than by `;`, so that the Entities a filter selects are more often several than the anchor
/// alone.
fn drawn_q(entities: &[DrawnEntity], position: usize, state: DrawnState) -> BoxedStrategy<DrawnQuery> {
    Union::new_weighted(vec![(ANCHORED, anchored_term(entities, position, state)), (FREE, free_term())])
        .prop_recursive(2, 4, 2, |inner| {
            (vec(inner, 2..3), prop_oneof![2 => Just(";"), 3 => Just("|")]).prop_map(|(members, operator)| DrawnQuery::grouped(&members, operator))
        })
        .boxed()
}

/// The geoquery a filter drawn from the Entity at `position` of `entities` in `state` carries, None most often and
/// always when the Entity holds no `GeoProperty` in that state. Its reference geometry is most often the
/// Attribute's own.
fn drawn_geo_part(entities: &[DrawnEntity], position: usize, state: DrawnState) -> BoxedStrategy<Option<DrawnGeoPart>> {
    let held: Vec<(&'static str, PooledGeometry)> = entities
        .get(position)
        .into_iter()
        .flat_map(|anchor| anchor.held(state))
        .filter_map(|attribute| attribute.family.geometry().map(|geometry| (attribute.name, geometry)))
        .collect();
    if held.is_empty() {
        return Just(None).boxed();
    }
    let part = (select(held), drawn_relation()).prop_flat_map(|((geoproperty, own), relation)| {
        prop_oneof![2 => Just(own), 1 => drawn_reference(relation, &[own], &[own])].prop_map(move |geometry| {
            Some(DrawnGeoPart {
                relation,
                geometry,
                geoproperty,
            })
        })
    });
    prop_oneof![2 => Just(None), 1 => part].boxed()
}

/// Up to `most` names of [`NAMES`], each once.
fn some_names(most: usize) -> BoxedStrategy<Vec<&'static str>> {
    vec(select(&NAMES[..]), 0..=most).prop_map(union).boxed()
}

/// A filter over `entities`, drawn from the one at `anchor`: its `q`, the geoquery it may carry, and the names
/// `jsonKeys` and `expandValues` list beside those its terms need.
///
/// The anchor is read after its creation more often than after its change. A term drawn from the change reads an
/// Attribute the anchor does not yet hold after its creation more often than one drawn from the creation reads an
/// Attribute the change replaces, so the weights even out how often each round finds the anchor as drawn.
pub fn drawn_filter(entities: Arc<[DrawnEntity]>, anchor: usize) -> BoxedStrategy<DrawnFilter> {
    prop_oneof![3 => Just(DrawnState::AfterCreation), 2 => Just(DrawnState::AfterChange)]
        .prop_flat_map(move |state| {
            (
                drawn_q(&entities, anchor, state),
                drawn_geo_part(&entities, anchor, state),
                some_names(2),
                some_names(1),
            )
        })
        .prop_map(|(query, geo, json_keys, expand_values)| DrawnFilter {
            q: query.text,
            json_keys: union(query.json_keys.into_iter().chain(json_keys)),
            expand_values: union(query.expand_values.into_iter().chain(expand_values)),
            traversal: query.traversal,
            geo,
        })
        .boxed()
}
