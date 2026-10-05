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
//! AND and OR only: a `q` expression has no negation, a scope query is a
//! disjunction of conjunctions, and the three filters are conjoined. A
//! monotone formula over existential leaves evaluated over the union of the
//! parts is therefore the formula over, for each leaf, whether the leaf held
//! in any part — which is what lets each part be read on its own, wherever it
//! is held, and the truths be joined afterwards.

use crate::{
    entity_selection::EntitySelection,
    filter::{eval::predicate_holds_in, expression::Expr, predicate::Predicate},
    geo::query::GeoQuery,
    queried_entity::{attribute_reading::QueriedEntity, linked_entities::LinkedEntities, located_entity::LocatedEntity, scoped_entity::ScopedEntity},
    scope::{pattern::ScopePattern, query::ScopeQuery},
};

/// One existential leaf of a query's value, geo and scope filters.
#[derive(Debug, Clone, Copy)]
pub enum FilterLeaf<'q> {
    /// One Query Term of the `q` expression (clause 4.9).
    Term(&'q Predicate),
    /// The geo-query (clause 4.10).
    Geo(&'q GeoQuery),
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

/// The value, geo and scope filters of an [`EntitySelection`], read as a
/// formula over their leaves.
#[derive(Debug, Clone, Copy)]
pub struct SplitFilter<'q> {
    /// The `q` expression (clause 4.9), when the query carries one.
    q: Option<&'q Expr>,
    /// The geo-query (clause 4.10), when the query carries one.
    geo: Option<&'q GeoQuery>,
    /// The scope query (clause 4.19), when the query carries one.
    scope: Option<&'q ScopeQuery>,
}

impl<'q> SplitFilter<'q> {
    /// The filters of `selection` that clause 5.7.4.4 applies after the parts
    /// of an entity have been aggregated.
    #[must_use]
    pub const fn of(selection: &'q EntitySelection) -> SplitFilter<'q> {
        SplitFilter {
            q: selection.q_expr.as_ref(),
            geo: selection.geo.as_ref(),
            scope: selection.scope_q.as_ref(),
        }
    }

    /// Whether the query carries none of the three filters, in which case
    /// clause 5.7.4.4 makes S7 equal to S4 and nothing is evaluated.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.q.is_none() && self.geo.is_none() && self.scope.is_none()
    }

    /// Every leaf: the Query Terms of `q` in pre-order, then the geo-query,
    /// then every pattern of every scope alternative in order.
    pub fn leaves(&self) -> impl Iterator<Item = FilterLeaf<'q>> + use<'q> {
        let mut terms = Vec::new();
        if let Some(expr) = self.q {
            collect_terms(expr, &mut terms);
        }
        let geo = self.geo.map(FilterLeaf::Geo);
        let scope = self
            .scope
            .into_iter()
            .flat_map(|scope| scope.alternatives().flatten().map(FilterLeaf::ScopeRequirement));
        terms.into_iter().map(FilterLeaf::Term).chain(geo).chain(scope)
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
        let q = self.q.is_none_or(|expr| expression_holds(expr, truths, &mut cursor));
        let geo = self.geo.is_none_or(|_| next_truth(truths, &mut cursor));
        let scope = self.scope.is_none_or(|scope| {
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

    /// The truth of every leaf over one part of an entity, read in memory, the
    /// entities a Query Term's hops cross into read from `linked`.
    pub fn truths_of<'a, E, L>(&self, entity: E, linked: L) -> LeafTruths
    where
        E: QueriedEntity<'a> + LocatedEntity<'a> + ScopedEntity<'a>,
        L: LinkedEntities<'a>,
    {
        LeafTruths(
            self.leaves()
                .map(|leaf| match leaf {
                    FilterLeaf::Term(pred) => predicate_holds_in(pred, entity, linked),
                    FilterLeaf::Geo(geo) => geo.matches_entity(entity),
                    FilterLeaf::ScopeRequirement(pattern) => entity.scopes().any(|scope| pattern.matches(scope)),
                })
                .collect(),
        )
    }
}

/// Gather the Query Terms of `expr` in pre-order.
fn collect_terms<'q>(expr: &'q Expr, terms: &mut Vec<&'q Predicate>) {
    match expr {
        Expr::And(inner) | Expr::Or(inner) => inner.iter().for_each(|term| collect_terms(term, terms)),
        Expr::Pred(pred) => terms.push(pred),
    }
}

/// The truth of the leaf under `cursor`, advancing it.
fn next_truth(truths: &LeafTruths, cursor: &mut usize) -> bool {
    let held = truths.get(*cursor);
    *cursor += 1;
    held
}

/// Whether `expr` holds when its Query Terms, from `cursor` on in pre-order,
/// hold as `truths` says.
///
/// Every term advances the cursor whether or not the connective above it is
/// already decided, so no term is skipped once the outcome is known.
fn expression_holds(expr: &Expr, truths: &LeafTruths, cursor: &mut usize) -> bool {
    match expr {
        Expr::And(terms) => {
            let mut all = true;
            for term in terms {
                all &= expression_holds(term, truths, cursor);
            }
            all
        }
        Expr::Or(terms) => {
            let mut any = false;
            for term in terms {
                any |= expression_holds(term, truths, cursor);
            }
            any
        }
        Expr::Pred(_) => next_truth(truths, cursor),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_selection::EntitySelection,
        filter::{eval::evaluate, parser::parse},
        geo::query::GeoQuery,
        queried_entity::{
            attribute_reading::QueriedEntity,
            expanded_fixture::{FixtureEntity, FixtureInstance},
            linked_entities::NoLinkedEntities,
            located_entity::LocatedEntity,
            scoped_entity::ScopedEntity,
        },
        scope::query::ScopeQuery,
        split_filter::{FilterLeaf, LeafTruths, SplitFilter},
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, geo_rel::GeoRel, geometry::Geometry, position::Position};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath};
    use serde_json::{Value, json};
    use std::borrow::Cow;

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    /// A `within` query over a box around `(13.4, 52.5)`.
    fn within_berlin() -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Within,
            geometry: Geometry::from_geojson_value(&json!({
                "type": "Polygon",
                "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
            }))
            .expect("polygon parses"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        }
    }

    fn selection(q: Option<&str>, geo: Option<GeoQuery>, scope: Option<&str>) -> EntitySelection {
        EntitySelection::builder()
            .q_expr(q.map(|raw| parse(raw).expect("q parses")))
            .geo(geo)
            .scope_q(scope.map(|raw| ScopeQuery::parse(raw).expect("scopeQ parses")))
            .build()
    }

    /// One part of an entity: its attributes, its `location` points and its
    /// scopes.
    struct Part {
        entity: FixtureEntity,
        location: Vec<GeometryForm>,
        scopes: Vec<ScopePath>,
    }

    impl<'a> QueriedEntity<'a> for &'a Part {
        type Instance = &'a FixtureInstance;

        fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = &'a FixtureInstance> {
            self.entity.attribute_instances(attribute)
        }

        fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
            self.entity.types()
        }
    }

    impl<'a> LocatedEntity<'a> for &'a Part {
        fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
            let location = attribute.as_str() == LOCATION;
            self.location.iter().filter(move |_| location).map(Cow::Borrowed)
        }
    }

    impl<'a> ScopedEntity<'a> for &'a Part {
        fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
            self.scopes.iter()
        }
    }

    fn part(speed: Option<u32>, point: Option<[f64; 2]>, scopes: &[&str]) -> Part {
        let mut document = json!({"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.com/Vehicle"]});
        if let (Some(speed), Value::Object(members)) = (speed, &mut document) {
            members.insert(
                "https://example.com/speed".into(),
                json!([{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": speed}]}]),
            );
        }
        Part {
            entity: FixtureEntity::from_expanded(&document),
            location: point
                .map(|[longitude, latitude]| GeometryForm::Point(Position::try_new(longitude, latitude, None).expect("a CRS84 position")))
                .into_iter()
                .collect(),
            scopes: scopes.iter().map(|path| path.parse().expect("a scope path")).collect(),
        }
    }

    #[test]
    fn a_filter_naming_nothing_is_empty() {
        let selection = EntitySelection::default();
        let filter = SplitFilter::of(&selection);
        assert!(filter.is_empty());
        pretty_assertions::assert_eq!(filter.leaf_count(), 0);
        assert!(filter.holds(&LeafTruths::none(0)), "no filter holds for every entity");
    }

    #[test]
    fn the_leaves_follow_q_then_geo_then_every_scope_pattern() {
        let selection = selection(
            Some("\"https://example.com/a\">1|\"https://example.com/b\"<2;\"https://example.com/c\""),
            Some(within_berlin()),
            Some("/A;/B,/C"),
        );
        let leaves: Vec<String> = SplitFilter::of(&selection)
            .leaves()
            .map(|leaf| match leaf {
                FilterLeaf::Term(pred) => format!("term {}", pred.path().terminal().attr),
                FilterLeaf::Geo(geo) => format!("geo {}", geo.geoproperty),
                FilterLeaf::ScopeRequirement(pattern) => format!("scope {pattern}"),
            })
            .collect();
        pretty_assertions::assert_eq!(
            leaves,
            [
                "term https://example.com/a",
                "term https://example.com/b",
                "term https://example.com/c",
                "geo https://uri.etsi.org/ngsi-ld/location",
                "scope /A",
                "scope /B",
                "scope /C",
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
        let selection = selection(None, None, Some("/A;/B,/C"));
        let filter = SplitFilter::of(&selection);
        assert!(!filter.holds(&LeafTruths::from_vec(vec![true, false, false])));
        assert!(filter.holds(&LeafTruths::from_vec(vec![true, true, false])));
        assert!(filter.holds(&LeafTruths::from_vec(vec![false, false, true])));
    }

    #[test]
    fn a_disjunction_reads_every_term_in_place() {
        // `a|b;c` holds through `c` alone only when `b` held as well: the
        // cursor must still reach `c` after `a` decided the disjunction.
        let selection = selection(
            Some("\"https://example.com/a\"|\"https://example.com/b\";\"https://example.com/c\""),
            Some(within_berlin()),
            None,
        );
        let filter = SplitFilter::of(&selection);
        assert!(filter.holds(&LeafTruths::from_vec(vec![true, false, false, true])));
        assert!(!filter.holds(&LeafTruths::from_vec(vec![true, false, false, false])));
        assert!(!filter.holds(&LeafTruths::from_vec(vec![false, true, false, true])));
        assert!(filter.holds(&LeafTruths::from_vec(vec![false, true, true, true])));
    }

    #[test]
    fn in_memory_truths_match_whole_entity_evaluation() {
        let parts = [
            part(Some(60), Some([13.4, 52.5]), &["/Madrid", "/Spain"]),
            part(Some(40), Some([13.4, 52.5]), &["/Madrid"]),
            part(Some(60), Some([0.0, 0.0]), &["/Spain"]),
            part(None, None, &[]),
        ];
        let filters = [
            selection(Some("\"https://example.com/speed\">50"), Some(within_berlin()), Some("/Madrid;/Spain")),
            selection(
                Some("\"https://example.com/speed\">50|\"https://example.com/speed\"<45"),
                None,
                Some("/Madrid,/Spain"),
            ),
            selection(Some("\"https://example.com/speed\""), Some(within_berlin()), None),
            selection(None, None, Some("/Spain")),
        ];
        for (filter_index, selection) in filters.iter().enumerate() {
            let filter = SplitFilter::of(selection);
            for (part_index, part) in parts.iter().enumerate() {
                let whole = selection.q_expr.as_ref().is_none_or(|expr| evaluate(expr, part, NoLinkedEntities))
                    && selection.geo.as_ref().is_none_or(|geo| geo.matches_entity(part))
                    && selection.scope_q.as_ref().is_none_or(|scope| scope.matches_entity(part));
                pretty_assertions::assert_eq!(
                    filter.holds(&filter.truths_of(part, NoLinkedEntities)),
                    whole,
                    "filter {filter_index} over part {part_index}"
                );
            }
        }
    }
}
