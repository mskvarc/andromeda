//! The value, geo and scope filters of a query, decided term by term over an
//! entity held in several parts (clause 5.7.4.4 `splitEntities`).
//!
//! Clause 5.7.4.4 applies those filters "after the Entity information from
//! different Context Sources and local information ... has been aggregated".
//! Every leaf of the three filters is existential over the instances it reads:
//! a Query Term holds when one target among the instances matches (clause
//! 4.9), the geo-query when one instance of the `GeoProperty` meets the
//! relation (clause 4.10), and a scope requirement when one of the entity's
//! scopes satisfies it (clause 4.19). The connectives joining the leaves are
//! AND and OR only: a `q` filter has no negation, a scope query is a
//! disjunction of conjunctions, and the three filters are conjoined. A
//! monotone formula over existential leaves evaluated over the union of the
//! parts is therefore the formula over, for each leaf, whether the leaf held
//! over some of the instances it reads.
//!
//! For the instances inside the window that is whether the leaf held in any
//! part, so each part is read on its own, wherever it is held, and the truths
//! are joined afterwards. The Scope in force when the window opens (clause
//! 4.18) is the latest across every part, which no part decides alone, so the
//! Scope events of every part are merged into one entity first and its truths
//! joined with the others.

use crate::{
    entity_filters::EntityFilters,
    entity_selection::EntitySelection,
    filter::reading::{filter_reading::FilterReading, hop::Hop, term_reading::TermReading},
    geo::compiled_geo_query::CompiledGeoQuery,
    scope::scope_pattern::ScopePattern,
};
use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;

/// One existential leaf of a query's value, geo and scope filters.
#[derive(Debug, Clone, Copy)]
pub enum FilterLeaf<'q> {
    /// One Query Term of the `q` filter in normal form (clause 4.9), which a
    /// store renders and the in-memory execution runs.
    Term(&'q TermReading),
    /// The geo-query (clause 4.10).
    Geo(&'q CompiledGeoQuery),
    /// One requirement of one alternative of the scope query (clause 4.19).
    ScopeRequirement(&'q ScopePattern),
}

/// The truth of every leaf of a [`SplitFilter`] over one part of an entity, in
/// the order [`SplitFilter::leaves`] yields the leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafTruths(Vec<bool>);

impl LeafTruths {
    /// `count` leaves, none of which holds: the truths of a part that holds
    /// nothing at all.
    #[must_use]
    pub fn none(count: usize) -> LeafTruths {
        LeafTruths(vec![false; count])
    }

    /// The truths read off a part, in leaf order.
    #[must_use]
    pub const fn from_vec(truths: Vec<bool>) -> LeafTruths {
        LeafTruths(truths)
    }

    /// Join the truths of another part of the same entity: a leaf holds over
    /// both parts when it holds over either.
    pub fn or(&mut self, other: &LeafTruths) {
        if self.0.len() < other.0.len() {
            self.0.resize(other.0.len(), false);
        }
        self.0.iter_mut().zip(&other.0).for_each(|(held, other)| *held |= *other);
    }

    /// Whether the leaf at `index` holds; a leaf the truths do not cover does
    /// not.
    #[must_use]
    pub fn get(&self, index: usize) -> bool {
        self.0.get(index).copied().unwrap_or(false)
    }
}

/// What the entities one hop depth reaches are read for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HopReads<'q> {
    /// The Relationships crossed at the next depth and the attributes a Query
    /// Term ends in at this one, without repetition.
    pub attributes: Vec<&'q AttributeIri>,
}

/// The value, geo and scope filters of an [`EntitySelection`], read as a
/// formula over their leaves.
#[derive(Debug, Clone, Copy)]
pub struct SplitFilter<'q> {
    /// The filters the formula is read from.
    filters: EntityFilters<'q>,
}

impl<'q> SplitFilter<'q> {
    /// The filters of `selection` that clause 5.7.4.4 applies after the parts
    /// of an entity have been aggregated.
    #[must_use]
    pub const fn of(selection: &'q EntitySelection) -> SplitFilter<'q> {
        SplitFilter {
            filters: EntityFilters::of(selection),
        }
    }

    /// Whether the query carries none of the three filters, in which case
    /// clause 5.7.4.4 makes S7 equal to S4 and nothing is evaluated.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.filters.q.is_none() && self.filters.geo.is_none() && self.filters.scope.is_none()
    }

    /// Every leaf: the Query Terms of `q` in normal form, in pre-order, then the
    /// geo-query, then every pattern of every scope alternative in order.
    pub fn leaves(&self) -> impl Iterator<Item = FilterLeaf<'q>> + use<'q> {
        let terms = self.filters.q.into_iter().flat_map(|q| q.reading().terms()).map(FilterLeaf::Term);
        let geo = self.filters.geo.map(FilterLeaf::Geo);
        let scope = self
            .filters
            .scope
            .into_iter()
            .flat_map(|scope| scope.alternatives().flatten().map(FilterLeaf::ScopeRequirement));
        terms.chain(geo).chain(scope)
    }

    /// The Relationships the Query Terms' hops cross, by depth: the first
    /// entry holds those an entity under evaluation is read through, the next
    /// those of the entities they link to, and so on, each without repetition.
    /// Empty when no Query Term crosses a hop.
    #[must_use]
    pub fn hop_relationships(&self) -> Vec<Vec<&'q AttributeIri>> {
        let mut levels: Vec<Vec<&'q AttributeIri>> = Vec::new();
        for leaf in self.leaves() {
            let FilterLeaf::Term(term) = leaf else {
                continue;
            };
            for (depth, hop) in term.hops().iter().enumerate() {
                if levels.len() <= depth {
                    levels.push(Vec::new());
                }
                if let Some(level) = levels.get_mut(depth)
                    && !level.contains(&hop.relationship())
                {
                    level.push(hop.relationship());
                }
            }
        }
        levels
    }

    /// What the entities each hop depth reaches are read for, parallel to
    /// [`Self::hop_relationships`].
    #[must_use]
    pub fn hop_reads(&self) -> Vec<HopReads<'q>> {
        let mut levels: Vec<HopReads<'q>> = Vec::new();
        for leaf in self.leaves() {
            let FilterLeaf::Term(term) = leaf else {
                continue;
            };
            let hops = term.hops();
            for depth in 0..hops.len() {
                if levels.len() <= depth {
                    levels.push(HopReads { attributes: Vec::new() });
                }
                let read = hops.get(depth + 1).map_or(term.attribute(), Hop::relationship);
                if let Some(level) = levels.get_mut(depth)
                    && !level.attributes.contains(&read)
                {
                    level.attributes.push(read);
                }
            }
        }
        levels
    }

    /// Whether a leaf reads the entity's Scope.
    #[must_use]
    pub const fn reads_scope(&self) -> bool {
        self.filters.scope.is_some()
    }

    /// How many leaves [`Self::leaves`] yields.
    #[must_use]
    pub fn leaf_count(&self) -> usize {
        self.leaves().count()
    }

    /// Whether the filters hold for an entity whose leaves hold as `truths`
    /// says.
    #[must_use]
    pub fn holds(&self, truths: &LeafTruths) -> bool {
        let mut cursor = 0;
        let q = self.filters.q.is_none_or(|q| reading_holds(q.reading(), truths, &mut cursor));
        let geo = self.filters.geo.is_none_or(|_| next_truth(truths, &mut cursor));
        let scope = self.filters.scope.is_none_or(|scope| {
            // Every requirement takes its place in the leaf order whether or
            // not an earlier alternative already held, so the walk folds
            // rather than short-circuiting.
            scope.alternatives().fold(false, |any, alternative| {
                let all = alternative.fold(true, |all, _| next_truth(truths, &mut cursor) && all);
                all || any
            })
        });
        q && geo && scope
    }

    /// The filters the formula is read from.
    #[must_use]
    pub const fn filters(&self) -> EntityFilters<'q> {
        self.filters
    }
}

/// The truth of the leaf under `cursor`, advancing it.
fn next_truth(truths: &LeafTruths, cursor: &mut usize) -> bool {
    let held = truths.get(*cursor);
    *cursor += 1;
    held
}

/// Whether `reading` holds when its Query Terms, from `cursor` on in
/// pre-order, hold as `truths` says.
///
/// Every term advances the cursor whether or not the connective above it is
/// already decided, so no term is skipped once the outcome is known.
fn reading_holds(reading: &FilterReading, truths: &LeafTruths, cursor: &mut usize) -> bool {
    match reading {
        FilterReading::All(children) => {
            let mut all = true;
            for child in children {
                all &= reading_holds(child, truths, cursor);
            }
            all
        }
        FilterReading::Any(children) => {
            let mut any = false;
            for child in children {
                any |= reading_holds(child, truths, cursor);
            }
            any
        }
        FilterReading::Term(_) => next_truth(truths, cursor),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_selection::EntitySelection,
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching},
        geo::compiled_geo_query::CompiledGeoQuery,
        name_stage::{Expanded, Written},
        namespace_expander::EXAMPLE_NAMESPACE,
        scope::scope_query::ScopeQuery,
        split_filter::{FilterLeaf, LeafTruths, SplitFilter},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
    use serde_json::json;

    /// `q` expanded under the example namespace.
    fn expanded(q: &str) -> Filter<Expanded> {
        EXAMPLE_NAMESPACE
            .expanded(q.parse::<Filter<Written>>().expect("a clause 4.9 filter"))
            .expect("the names expand")
    }

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    /// A `within` query over a box around `(13.4, 52.5)`.
    fn within_berlin() -> CompiledGeoQuery {
        CompiledGeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(json!({
                "type": "Polygon",
                "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
            }))
            .expect("polygon parses"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        }
    }

    fn selection(q: Option<&str>, geo: Option<CompiledGeoQuery>, scope: Option<&str>) -> EntitySelection {
        EntitySelection::builder()
            .q(q.map(|raw| CompiledFilter::new(expanded(raw), LanguageMatching::TagEquality)))
            .geo(geo)
            .scope_q(scope.map(|raw| raw.parse::<ScopeQuery>().expect("scopeQ parses")))
            .build()
    }

    #[test]
    fn a_filter_naming_nothing_is_empty() {
        let selection = EntitySelection::default();
        let filter = SplitFilter::of(&selection);
        assert!(filter.is_empty());
        pretty_assertions::assert_eq!(filter.leaf_count(), 0);
        assert!(filter.holds(&LeafTruths::none(0)), "no filter holds for every entity");
    }

    /// The terms come in the normal form's order, which puts the group
    /// `b<2;c` before the term `a>1`.
    #[test]
    fn the_leaves_follow_q_then_geo_then_every_scope_pattern() {
        let selection = selection(
            Some("\"https://example.com/a\">1|\"https://example.com/b\"<2;\"https://example.com/c\""),
            Some(within_berlin()),
            Some("(/A;/B),/C"),
        );
        let leaves: Vec<String> = SplitFilter::of(&selection)
            .leaves()
            .map(|leaf| match leaf {
                FilterLeaf::Term(term) => format!("term {}", term.attribute()),
                FilterLeaf::Geo(geo) => format!("geo {}", geo.geoproperty),
                FilterLeaf::ScopeRequirement(pattern) => format!("scope {pattern}"),
            })
            .collect();
        pretty_assertions::assert_eq!(
            leaves,
            [
                "term https://example.com/b",
                "term https://example.com/c",
                "term https://example.com/a",
                "geo https://uri.etsi.org/ngsi-ld/location",
                "scope /A",
                "scope /B",
                "scope /C",
            ]
        );
    }

    #[test]
    fn only_a_scope_query_reads_the_scope() {
        let with_scope = selection(Some("\"https://example.com/speed\">1"), Some(within_berlin()), Some("/A"));
        assert!(SplitFilter::of(&with_scope).reads_scope());
        let without = selection(Some("\"https://example.com/speed\">1"), Some(within_berlin()), None);
        assert!(!SplitFilter::of(&without).reads_scope());
    }

    #[test]
    fn the_hop_relationships_are_grouped_by_depth() {
        let selection = selection(
            Some(
                "\"https://example.com/parkedIn\"{\"https://example.com/operator\"{\"https://example.com/name\"}}==\"x\"|\"https://example.com/parkedIn\"{\"https://example.com/free\"}>0;\"https://example.com/speed\">1",
            ),
            None,
            None,
        );
        let levels: Vec<Vec<&str>> = SplitFilter::of(&selection)
            .hop_relationships()
            .into_iter()
            .map(|level| level.into_iter().map(AttributeIri::as_str).collect())
            .collect();
        pretty_assertions::assert_eq!(levels, [vec!["https://example.com/parkedIn"], vec!["https://example.com/operator"]]);
    }

    /// The normal form reads the group holding the `free` hop before the term
    /// holding the `operator` hop.
    #[test]
    fn the_hop_reads_are_the_next_relationships_and_the_attributes_the_terms_end_in() {
        let selection = selection(
            Some(
                "\"https://example.com/parkedIn\"{\"https://example.com/operator\"{\"https://example.com/name\"}}==\"x\"|\"https://example.com/parkedIn\"{\"https://example.com/free\"}>0;\"https://example.com/speed\">1",
            ),
            None,
            None,
        );
        let levels = SplitFilter::of(&selection).hop_reads();
        let reads: Vec<Vec<&str>> = levels.iter().map(|level| level.attributes.iter().map(|name| name.as_str()).collect()).collect();
        pretty_assertions::assert_eq!(
            reads,
            [
                vec!["https://example.com/free", "https://example.com/operator"],
                vec!["https://example.com/name"]
            ]
        );
    }

    #[test]
    fn a_conjunction_holds_when_each_term_held_in_a_different_part() {
        let selection = selection(Some("\"https://example.com/speed\">50"), Some(within_berlin()), None);
        let filter = SplitFilter::of(&selection);
        let local = LeafTruths::from_vec(vec![true, false]);
        let remote = LeafTruths::from_vec(vec![false, true]);
        assert!(!filter.holds(&local));
        assert!(!filter.holds(&remote));
        let mut merged = local;
        merged.or(&remote);
        assert!(filter.holds(&merged));
    }

    #[test]
    fn a_scope_alternative_needs_every_requirement() {
        let selection = selection(None, None, Some("(/A;/B),/C"));
        let filter = SplitFilter::of(&selection);
        assert!(!filter.holds(&LeafTruths::from_vec(vec![true, false, false])));
        assert!(filter.holds(&LeafTruths::from_vec(vec![true, true, false])));
        assert!(filter.holds(&LeafTruths::from_vec(vec![false, false, true])));
    }

    #[test]
    fn a_disjunction_reads_every_term_in_place() {
        // `a|b;c` holds through `a` alone or through `b` and `c` together. The
        // normal form reads the group `b;c` before `a`, so the cursor must
        // still step past `c` after `b` decided the group, to reach `a`.
        let selection = selection(
            Some("\"https://example.com/a\"|\"https://example.com/b\";\"https://example.com/c\""),
            Some(within_berlin()),
            None,
        );
        let filter = SplitFilter::of(&selection);
        assert!(filter.holds(&LeafTruths::from_vec(vec![false, false, true, true])));
        assert!(!filter.holds(&LeafTruths::from_vec(vec![false, false, true, false])));
        assert!(!filter.holds(&LeafTruths::from_vec(vec![true, false, false, true])));
        assert!(filter.holds(&LeafTruths::from_vec(vec![true, true, false, true])));
    }
}
