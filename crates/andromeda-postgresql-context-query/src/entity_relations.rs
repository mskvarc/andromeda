//! Which relations an entity query statement reads, and which one drives it.
//!
//! A statement is driven by `entities`, which carries the identifier the
//! selectors filter and the ordering breaks ties on, unless the request selects
//! by type alone: then it is driven by the type-membership walk of that
//! selection (clause 4.17), which carries the same `entity_key` and `id` and
//! yields a type's Entities in identifier order without visiting any other.
//! `entities` is then joined only for the `createdAt` or `modifiedAt` a sort
//! reads. The walk of a selection of several alternatives is one stream per
//! alternative, each a range in identifier order holding the alternative's
//! Entities that no alternative before it selects. A statement that reads
//! every Entity of the walk, or the few its filters narrow it to, reads their
//! union, in no order. A statement that stops at its first rows in page order
//! reads each stream up to those rows and merges what they yield
//! ([`EntityRelations::streams`]): PostgreSQL merges no two ranges of one index
//! in order, so the union in page order sorts every member of the types
//! before its first row. A geo-query keeps `entities` driving a statement
//! narrowed to its matches, as the plain page and the index branch of a geo
//! page are. Its matches come from the spatial index as a set of keys, so the
//! statement starts from them and tests the type on the search row the
//! geo-query joins anyway. The walks of a geo page probe each
//! Entity they visit for the geo-query instead, so they take the membership
//! walk when the selectors are types alone, and join the search row only for
//! the other filters that read it. `entity_search` is joined only when some
//! axis of the query reads an indexed column of it. A `q` expression and a
//! sort on an attribute value read `current_attributes` rows through probes
//! and laterals correlated on the entity key, so they join nothing. What the
//! response holds is not a reason to join either: the head of each Entity is
//! looked up by key for the rows of the cut page alone, and its attributes are
//! read afterwards by the keys of the page. A query that asks for identifiers
//! with no filter joins nothing and reads one relation.

use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::{ConsideredEntities, EntitySelection},
    ordering::{compiled_ordering_term::CompiledOrderingTerm, expanded_ordering_target::ExpandedOrderingTarget},
    standing_instant::StandingInstant,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    standing_instant::instant_of,
    type_membership::{disjoint_members_of_any, drives},
};
use andromeda_postgresql_schema::{
    current::{Entities, EntitySearch},
    type_membership::MembershipSide,
};
use andromeda_postgresql_seaquery::branch::qualified;
use andromeda_postgresql_stored_form::{
    entity_sides::{holds_standing_state, not_expired},
    memberships::Memberships,
};
use andromeda_postgresql_stored_values::resolved_names::ResolvedNames;
use sea_query::{Asterisk, Condition, DynIden, Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, UnionType};

/// The type-membership walk a type-driven statement reads, `selected`.
#[derive(Iden, Clone, Copy)]
#[iden = "selected"]
pub struct Selected;

/// Whether a relation takes part in the statement, and how it meets the
/// driving row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demand {
    /// The relation is joined to the driving row on `entity_key`.
    Joined,
    /// The relation's row of each driving row is read on its own, in a
    /// lateral subquery behind `OFFSET 0` keyed on that row's `entity_key`,
    /// which the planner cannot flatten into a join of the two relations.
    JoinedPerRow,
    /// The relation is not named in the statement.
    NotJoined,
}

/// The relation a statement is driven by.
#[derive(Debug, Clone, PartialEq)]
pub enum RowSource {
    /// `entities`, walked or probed as the filters choose.
    Entities,
    /// The type-membership walk of the request's type selection, read under
    /// [`Selected`], with `entities` joined as `entities` says: one stream of
    /// `(entity_key, id)` per alternative of the selection, each in identifier
    /// order and holding the alternative's Entities that no alternative
    /// before it selects.
    TypeMembers {
        /// The first alternative's stream.
        first: Box<SelectStatement>,
        /// The stream of each alternative after the first, none for a
        /// selection of one alternative.
        rest: Vec<SelectStatement>,
        /// Whether `entities` is joined for a lifecycle column.
        entities: Demand,
    },
}

/// How many streams drive a statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCount {
    /// One: `entities`, or the type walk of a selection of one alternative.
    One,
    /// Several: the type walk of a selection of several alternatives, one
    /// stream per alternative.
    Several,
}

/// The relations of one entity statement.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRelations {
    /// The relation that drives the statement.
    pub source: RowSource,
    /// Whether `entity_search` is joined.
    pub search: Demand,
}

impl EntityRelations {
    /// The relations a compiled query reads, in the branch `schema`.
    ///
    /// The search row is joined for a type selector the membership walk does
    /// not answer, an `attrs` term, a projection that can leave an entity no
    /// member, a geo query, a scope query, an ordering on `type` or `scope`, or
    /// a distance ordering measured to the default `location`.
    #[must_use]
    pub fn for_query(query: &CompiledEntityQuery, types: &ResolvedNames<EntityTypeName>, schema: Option<&DynIden>) -> EntityRelations {
        EntityRelations::reading(query, types, schema, GeoRole::Narrows)
    }

    /// The relations a walk that probes the geo-query reads, in the branch
    /// `schema`: those [`EntityRelations::for_query`] chooses with the
    /// geo-query left to a probe of each Entity the walk yields.
    ///
    /// The walk is the membership walk when the selectors are types alone, and
    /// the search row is joined only for the other filters that read it.
    #[must_use]
    pub fn for_geo_walk(query: &CompiledEntityQuery, types: &ResolvedNames<EntityTypeName>, schema: Option<&DynIden>) -> EntityRelations {
        EntityRelations::reading(query, types, schema, GeoRole::Probed)
    }

    /// The relations the fallback count of a paginated query reads: the same
    /// source and filters, no projection and no ordering.
    #[must_use]
    pub fn for_count(query: &CompiledEntityQuery, types: &ResolvedNames<EntityTypeName>, schema: Option<&DynIden>) -> EntityRelations {
        let selection = &query.selection;
        let source = source(selection, types, schema, Demand::NotJoined, GeoRole::Narrows);
        EntityRelations {
            search: demand(search_demand(selection, &source, GeoRole::Narrows)),
            source,
        }
    }

    /// The relations a statement over `query` reads in the branch `schema`,
    /// its geo-query playing `geo`.
    fn reading(query: &CompiledEntityQuery, types: &ResolvedNames<EntityTypeName>, schema: Option<&DynIden>, geo: GeoRole) -> EntityRelations {
        let selection = &query.selection;
        let terms = &query.ordering.terms;
        let lifecycle = demand(terms.iter().any(reads_lifecycle));
        let source = source(selection, types, schema, lifecycle, geo);
        let search = search_demand(selection, &source, geo) || terms.iter().any(reads_search_row);
        EntityRelations {
            source,
            search: demand(search),
        }
    }

    /// How the statement's fragments reach the rows, in the given branch
    /// schema and standing at `standing`, with the key and identifier read
    /// from the membership walk when it drives the statement.
    #[must_use]
    pub fn rows(&self, schema: Option<DynIden>, standing: StandingInstant) -> EntityRowRefs {
        let rows = EntityRowRefs::unaliased(schema).standing_at(standing);
        match self.source {
            RowSource::Entities => rows,
            RowSource::TypeMembers { .. } => rows.keyed_by(Selected),
        }
    }

    /// These relations as a walk of `query`'s selection in page order reads
    /// them: the search row joined to each walked Entity on its own, and only
    /// when the walk's selectors or sort keys read it. Every other filter that
    /// reads it looks it up for each Entity the walk yields.
    ///
    /// A walk fenced by `OFFSET 0` alone is planned to yield every row, so a
    /// search row joined to it plainly makes it a merge join of every driving
    /// row with every search row, sorted by identifier before the first is
    /// probed. Joined per Entity, the walk stays an ordered scan of its index.
    #[must_use]
    pub fn walked(&self, query: &CompiledEntityQuery) -> EntityRelations {
        let read = selected_by_search_row(&query.selection, &self.source) || query.ordering.terms.iter().any(reads_search_row);
        let search = match (self.search, read) {
            (Demand::Joined | Demand::JoinedPerRow, true) => Demand::JoinedPerRow,
            (Demand::Joined | Demand::JoinedPerRow, false) | (Demand::NotJoined, true | false) => Demand::NotJoined,
        };
        EntityRelations {
            source: self.source.clone(),
            search,
        }
    }

    /// How many streams drive these relations.
    #[must_use]
    pub const fn stream_count(&self) -> StreamCount {
        match &self.source {
            RowSource::TypeMembers { rest, .. } if !rest.is_empty() => StreamCount::Several,
            RowSource::TypeMembers { .. } | RowSource::Entities => StreamCount::One,
        }
    }

    /// These relations one stream at a time, the first and the rest: one per
    /// stream of a type walk of several alternatives
    /// ([`RowSource::TypeMembers`]), each driven by its stream alone, and
    /// these relations alone otherwise.
    ///
    /// The streams are disjoint, so the first `n` Entities of the selection
    /// in page order are among the first `n` of each stream in that order.
    #[must_use]
    pub fn streams(self) -> (EntityRelations, Vec<EntityRelations>) {
        let search = self.search;
        match self.source {
            RowSource::TypeMembers { first, rest, entities } => {
                let driven_by = |stream: Box<SelectStatement>| EntityRelations {
                    source: RowSource::TypeMembers {
                        first: stream,
                        rest: Vec::new(),
                        entities,
                    },
                    search,
                };
                let rest = rest.into_iter().map(|stream| driven_by(Box::new(stream))).collect();
                (driven_by(first), rest)
            }
            RowSource::Entities => (self, Vec::new()),
        }
    }

    /// Add the driving relation and the joins this demand asks for to
    /// `select`, every relation qualified by the branch schema (`None` for the
    /// live branch), the driving Entity standing at `standing`.
    pub fn apply_from(self, select: &mut SelectStatement, schema: Option<&DynIden>, standing: StandingInstant) {
        let driving_key = self.rows(schema.cloned(), standing).entity_key_column();
        match self.source {
            RowSource::Entities => {
                // An identity row whose current state a core deletion removed
                // keeps only its Temporal Evolution, which no core read sees,
                // and an Entity past its `expiresAt` is invalid (clause 4.22).
                select
                    .from(qualified(schema, Entities::Table))
                    .and_where(holds_standing_state(Entities::Table, instant_of(standing)));
            }
            RowSource::TypeMembers { first, rest, entities } => {
                // The streams are disjoint, so their union holds each Entity
                // once, in no order.
                let walk = rest.into_iter().fold(*first, |mut union, stream| {
                    union.union(UnionType::All, stream);
                    union
                });
                select.from_subquery(walk, Selected);
                join_on_key(select, entities, schema, (Entities::Table, Entities::EntityKey), &driving_key);
                // The walk reads current-side memberships alone, so only the
                // clause 4.22 expiry is left to test.
                select.and_where(not_expired(driving_key.clone(), schema, instant_of(standing)));
            }
        }
        join_on_key(select, self.search, schema, (EntitySearch::Table, EntitySearch::EntityKey), &driving_key);
    }
}

/// What a selection's geo-query is to the statement its relations serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeoRole {
    /// Its matches narrow the selection, read from the spatial index as a set
    /// of keys.
    Narrows,
    /// Each Entity the statement yields is probed for it.
    Probed,
}

/// Whether `selection` is narrowed to the matches of a geo-query playing
/// `geo`.
const fn narrowed_by_geo(selection: &EntitySelection, geo: GeoRole) -> bool {
    match geo {
        GeoRole::Narrows => selection.geo.is_some(),
        GeoRole::Probed => false,
    }
}

/// The relation a selection is driven by: the membership walk when every
/// selector selects by a type selection that has a range to walk and names no
/// identifier, and no geo-query playing `geo` narrows the selection,
/// `entities` otherwise.
///
/// A selector naming identifiers is already answered by the identifier index,
/// a geo-query's matches by the spatial index, and a request that considers no
/// Entity (clause 5.5.14) walks nothing.
fn source(selection: &EntitySelection, types: &ResolvedNames<EntityTypeName>, schema: Option<&DynIden>, lifecycle: Demand, geo: GeoRole) -> RowSource {
    let walkable = match selection.considered {
        ConsideredEntities::Selected => {
            !narrowed_by_geo(selection, geo)
                && !selection.selectors.is_empty()
                && selection
                    .selectors
                    .iter()
                    .all(|selector| selector.ids.is_empty() && selector.type_selection.as_ref().is_some_and(drives))
        }
        ConsideredEntities::NoEntity => false,
    };
    let selections = selection.selectors.iter().filter_map(|selector| selector.type_selection.as_ref());
    let memberships = Memberships {
        side: MembershipSide::Current,
        schema,
    };
    match walkable.then(|| disjoint_members_of_any(selections, types, memberships)).flatten() {
        Some((first, rest)) => RowSource::TypeMembers {
            first: Box::new(first),
            rest,
            entities: lifecycle,
        },
        None => RowSource::Entities,
    }
}

/// Whether a selection's filters read the search row: a type selector the
/// membership walk does not answer, a geo-query playing `geo` that narrows the
/// selection, or any other search axis.
fn search_demand(selection: &EntitySelection, source: &RowSource, geo: GeoRole) -> bool {
    selected_by_search_row(selection, source)
        || !selection.attrs.is_empty()
        || selection.projected_members.constrains()
        || narrowed_by_geo(selection, geo)
        || selection.scope_q.is_some()
}

/// Whether the selectors of `selection` read the search row of the Entities
/// `source` drives: a type selector the membership walk does not answer.
fn selected_by_search_row(selection: &EntitySelection, source: &RowSource) -> bool {
    match source {
        RowSource::Entities => selection.selectors.iter().any(|selector| selector.type_selection.is_some()),
        RowSource::TypeMembers { .. } => false,
    }
}

/// Join `table`, in the branch `schema`, on its `entity_key` column `key` to
/// the driving row's `driving_key`, as `demand` asks.
fn join_on_key<T: IntoIden + Copy>(select: &mut SelectStatement, demand: Demand, schema: Option<&DynIden>, (table, key): (T, T), driving_key: &SimpleExpr) {
    match demand {
        Demand::Joined => {
            select.join(JoinType::Join, qualified(schema, table), Expr::col((table, key)).eq(driving_key.clone()));
        }
        Demand::JoinedPerRow => {
            let mut row = Query::select();
            row.column(Asterisk)
                .from(qualified(schema, table))
                .and_where(Expr::col((table, key)).eq(driving_key.clone()))
                .offset(0);
            select.join_lateral(JoinType::InnerJoin, row.take(), table, Condition::all());
        }
        Demand::NotJoined => {}
    }
}

/// `Joined` when `needed`.
const fn demand(needed: bool) -> Demand {
    if needed { Demand::Joined } else { Demand::NotJoined }
}

/// Whether an ordering term reads a lifecycle column of the `entities` row
/// other than the key and the identifier the membership walk carries.
const fn reads_lifecycle(term: &CompiledOrderingTerm) -> bool {
    match &term.target {
        ExpandedOrderingTarget::CreatedAt | ExpandedOrderingTarget::ModifiedAt => true,
        ExpandedOrderingTarget::Id | ExpandedOrderingTarget::Type | ExpandedOrderingTarget::Scope | ExpandedOrderingTarget::Attribute(_) => false,
    }
}

/// Whether an ordering term reads a column of the search row.
fn reads_search_row(term: &CompiledOrderingTerm) -> bool {
    match &term.target {
        ExpandedOrderingTarget::Type | ExpandedOrderingTarget::Scope => true,
        // Only a term naming the attribute itself can read the dedicated
        // geometry column; a path descending into it reads the attribute's rows.
        ExpandedOrderingTarget::Attribute(path) => term.direction.is_distance() && path.is_simple() && path.attribute.as_str() == LOCATION.as_str(),
        // `createdAt` and `modifiedAt` read the identity row.
        ExpandedOrderingTarget::Id | ExpandedOrderingTarget::CreatedAt | ExpandedOrderingTarget::ModifiedAt => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_relations::{Demand, EntityRelations, RowSource};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::{
        entity::compiled::CompiledEntityQuery,
        entity_constraint::EntityConstraint,
        entity_selection::{ConsideredEntities, EntitySelection},
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching, value_path::ValuePath},
        geo::compiled_geo_query::CompiledGeoQuery,
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
        ordering::{compiled_ordering_term::CompiledOrderingTerm, expanded_ordering_target::ExpandedOrderingTarget, ordering_direction::OrderingDirection},
        pattern::Pattern,
        scope::scope_query::ScopeQuery,
        standing_instant::StandingInstant,
        type_selection::TypeSelection,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_schema::current::Entities;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{Query, SelectStatement};

    fn empty_query() -> CompiledEntityQuery {
        CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build()
    }

    fn term(target: ExpandedOrderingTarget, direction: OrderingDirection) -> CompiledOrderingTerm {
        CompiledOrderingTerm {
            target,
            direction,
            collation: None,
        }
    }

    fn joined(search: Demand) -> EntityRelations {
        EntityRelations {
            source: RowSource::Entities,
            search,
        }
    }

    fn types() -> ResolvedNames<EntityTypeName> {
        ResolvedNames::from_pairs([(
            "https://example.com/T".parse::<EntityTypeName>().expect("valid type name"),
            EntityTypeNameKey::from(4),
        )])
    }

    fn relations(query: &CompiledEntityQuery) -> EntityRelations {
        EntityRelations::for_query(query, &types(), None)
    }

    fn typed_query() -> CompiledEntityQuery {
        let mut query = empty_query();
        query.selection.selectors = vec![
            EntityConstraint::builder()
                .type_selection(Some(TypeSelection::single(
                    IriBuf::new("https://example.com/T".to_string()).expect("valid IRI").into(),
                )))
                .build(),
        ];
        query
    }

    /// The Entities whose `location` lies within a point.
    fn within_point() -> CompiledGeoQuery {
        CompiledGeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point"),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        }
    }

    /// The FROM clause and joins `relations` renders, selecting the identifier
    /// the rows it keys read.
    fn from_clause(relations: EntityRelations) -> SelectStatement {
        let mut select = Query::select();
        select.expr(relations.rows(None, StandingInstant::WhenRead).identity_column(Entities::Id));
        relations.apply_from(&mut select, None, StandingInstant::WhenRead);
        select
    }

    #[test]
    fn an_identifier_query_with_no_filter_joins_nothing() {
        assert_eq!(relations(&empty_query()), joined(Demand::NotJoined));
        assert_statement_snapshot!(
            "entity-relations-an-identifier-query-with-no-filter-joins-nothing",
            from_clause(relations(&empty_query()))
        );
    }

    /// A `q` filter reads the rows of its attributes through probes on the
    /// entity key, so it joins nothing.
    #[test]
    fn a_q_filter_joins_nothing() {
        let mut filtered = empty_query();
        filtered.selection.q = Some(CompiledFilter::new(
            EXAMPLE_NAMESPACE
                .expanded("\"https://example.com/speed\">1".parse::<Filter<Written>>().expect("valid q"))
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        ));
        assert_statement_snapshot!("entity-relations-a-q-filter-joins-nothing", from_clause(relations(&filtered)));
    }

    #[test]
    fn every_search_axis_joins_the_search_row() {
        let mut attrs = empty_query();
        attrs.selection.attrs = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        let mut geo = empty_query();
        geo.selection.geo = Some(within_point());
        let mut scope = empty_query();
        scope.selection.scope_q = Some("/a".parse::<ScopeQuery>().expect("valid scope query"));
        let mut by_type = empty_query();
        by_type.ordering.terms = vec![term(ExpandedOrderingTarget::Type, OrderingDirection::Asc)];
        let mut by_distance = empty_query();
        by_distance.ordering.terms = vec![term(
            ExpandedOrderingTarget::Attribute(ValuePath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            OrderingDirection::DistAsc,
        )];
        let mut typed_by_identifier = typed_query();
        typed_by_identifier.selection.selectors[0].ids = vec!["urn:a".parse().expect("valid URI")];
        for query in [attrs, geo, scope, by_type, by_distance, typed_by_identifier] {
            assert_eq!(relations(&query).search, Demand::Joined, "{query:?}");
        }
    }

    /// Clause 4.17: a selection by type alone is driven by the membership walk,
    /// which answers the type itself, so neither `entities` nor the search row
    /// is joined and the identifier is read from the walk.
    #[test]
    fn a_selection_by_type_alone_is_driven_by_the_membership_walk() {
        let relations = relations(&typed_query());
        assert_eq!(relations.search, Demand::NotJoined);
        assert!(matches!(
            relations.source,
            RowSource::TypeMembers {
                entities: Demand::NotJoined,
                ..
            }
        ));
        assert_statement_snapshot!(
            "entity-relations-a-selection-by-type-alone-is-driven-by-the-membership-walk",
            from_clause(relations)
        );
    }

    /// An identifier pattern is a filter on the walk's identifier and keeps the
    /// walk; the wildcard, and a request that considers no Entity, walk nothing.
    #[test]
    fn a_pattern_keeps_the_walk_and_the_wildcard_or_no_entity_does_not() {
        let mut patterned = typed_query();
        patterned.selection.selectors[0].id_pattern = Some("urn:a.*".parse::<Pattern>().expect("valid pattern"));
        assert!(matches!(relations(&patterned).source, RowSource::TypeMembers { .. }));

        let mut wildcard = empty_query();
        wildcard.selection.selectors = vec![EntityConstraint::builder().type_selection(Some(TypeSelection::Wildcard)).build()];
        assert_eq!(relations(&wildcard).source, RowSource::Entities);

        let mut nothing = typed_query();
        nothing.selection.considered = ConsideredEntities::NoEntity;
        assert_eq!(relations(&nothing).source, RowSource::Entities);
    }

    /// A geo-query's matches drive the statement, so a typed geo-query is
    /// driven by `entities` and tests its types on the search row, one type or
    /// several.
    #[test]
    fn a_typed_geo_query_is_driven_by_entities_and_tests_the_type_on_the_search_row() {
        let mut one = typed_query();
        one.selection.geo = Some(within_point());
        let mut several = typed_query();
        several.selection.selectors.push(
            EntityConstraint::builder()
                .type_selection(Some(TypeSelection::single(
                    IriBuf::new("https://example.com/U".to_string()).expect("valid IRI").into(),
                )))
                .build(),
        );
        several.selection.geo = Some(within_point());
        for query in [one, several] {
            assert_eq!(relations(&query), joined(Demand::Joined), "{query:?}");
            assert_eq!(EntityRelations::for_count(&query, &types(), None), joined(Demand::Joined), "{query:?}");
        }
    }

    /// A `createdAt` sort reads a lifecycle column the walk does not carry, so
    /// `entities` is joined to the walk on its key.
    #[test]
    fn a_created_at_sort_joins_the_entities_row_to_the_walk() {
        let mut query = typed_query();
        query.ordering.terms = vec![term(ExpandedOrderingTarget::CreatedAt, OrderingDirection::Asc)];
        let relations = relations(&query);
        assert!(matches!(relations.source, RowSource::TypeMembers { entities: Demand::Joined, .. }));
        assert_statement_snapshot!("entity-relations-a-created-at-sort-joins-the-entities-row-to-the-walk", from_clause(relations));
    }

    /// Every relation a walked statement joins correlates on the walk's key.
    #[test]
    fn a_walked_statement_joins_on_the_walks_key() {
        let mut query = typed_query();
        query.selection.q = Some(CompiledFilter::new(
            EXAMPLE_NAMESPACE
                .expanded("\"https://example.com/speed\">1".parse::<Filter<Written>>().expect("valid q"))
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        ));
        query.selection.attrs = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        assert_statement_snapshot!("entity-relations-a-walked-statement-joins-on-the-walks-key", from_clause(relations(&query)));
    }

    /// An attribute value ordering reads the attribute's rows through a
    /// lateral on the entity key, so it joins nothing.
    #[test]
    fn an_attribute_ordering_joins_nothing() {
        let mut ordered = empty_query();
        ordered.ordering.terms = vec![term(
            ExpandedOrderingTarget::Attribute(ValuePath::simple("https://example.com/speed".parse().expect("valid attribute name"))),
            OrderingDirection::Asc,
        )];
        assert_eq!(relations(&ordered), joined(Demand::NotJoined));
    }

    /// `createdAt` and `modifiedAt` are columns of the identity row, so an
    /// ordering on either joins nothing beside it.
    #[test]
    fn a_system_time_ordering_joins_nothing() {
        for target in [ExpandedOrderingTarget::ModifiedAt, ExpandedOrderingTarget::CreatedAt] {
            let mut query = empty_query();
            query.ordering.terms = vec![term(target, OrderingDirection::Desc)];
            assert_eq!(relations(&query), joined(Demand::NotJoined));
        }
    }

    /// A `modifiedAt` sort reads the identity row, which a type-driven walk
    /// joins on its key, as it does for `createdAt`.
    #[test]
    fn a_modified_at_sort_joins_the_entities_row_to_the_walk() {
        let mut query = typed_query();
        query.ordering.terms = vec![term(ExpandedOrderingTarget::ModifiedAt, OrderingDirection::Asc)];
        assert!(matches!(relations(&query).source, RowSource::TypeMembers { entities: Demand::Joined, .. }));
    }

    #[test]
    fn a_selector_on_identifiers_alone_needs_no_search_row() {
        let mut query = empty_query();
        query.selection.selectors = vec![EntityConstraint::builder().ids(vec!["urn:a".parse().expect("valid URI")]).build()];
        assert_eq!(relations(&query).search, Demand::NotJoined);
    }

    #[test]
    fn the_count_joins_what_its_filters_read_and_never_the_projection() {
        let mut query = empty_query();
        query.selection.attrs = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        assert_eq!(EntityRelations::for_count(&query, &types(), None), joined(Demand::Joined));
    }

    /// The count of a walked selection counts the walk, and never joins
    /// `entities` for an ordering it does not have.
    #[test]
    fn the_count_of_a_walked_selection_counts_the_walk() {
        let mut query = typed_query();
        query.ordering.terms = vec![term(ExpandedOrderingTarget::CreatedAt, OrderingDirection::Asc)];
        let count = EntityRelations::for_count(&query, &types(), None);
        assert!(matches!(
            count.source,
            RowSource::TypeMembers {
                entities: Demand::NotJoined,
                ..
            }
        ));
    }

    #[test]
    fn the_joins_are_on_the_surrogate_key() {
        assert_statement_snapshot!("entity-relations-the-joins-are-on-the-surrogate-key", from_clause(joined(Demand::Joined)));
    }

    /// A type selector read off the search row of `entities` is a selector of
    /// the walk, so the walk joins the search row of each Entity it visits.
    #[test]
    fn a_walk_joins_the_search_row_of_each_entity_it_visits() {
        let walked = joined(Demand::Joined).walked(&typed_query());
        assert_eq!(walked, joined(Demand::JoinedPerRow));
        assert_statement_snapshot!("entity-relations-a-walk-joins-the-search-row-of-each-entity-it-visits", from_clause(walked));
    }

    /// An ordering on `type` sorts the walk by the search row.
    #[test]
    fn a_membership_walk_joins_the_search_row_of_each_member() {
        let mut query = typed_query();
        query.ordering.terms = vec![term(ExpandedOrderingTarget::Type, OrderingDirection::Asc)];
        let walked = relations(&query).walked(&query);
        assert_eq!(walked.search, Demand::JoinedPerRow);
        assert!(matches!(
            walked.source,
            RowSource::TypeMembers {
                entities: Demand::NotJoined,
                ..
            }
        ));
        assert_statement_snapshot!("entity-relations-a-membership-walk-joins-the-search-row-of-each-member", from_clause(walked));
    }

    /// The walks of a geo page probe each Entity for the geo-query, so a
    /// selection by type alone takes the membership walk and joins no search
    /// row, while a statement narrowed to the matches keeps `entities` driving
    /// and tests the type on the search row.
    #[test]
    fn a_geo_walk_reads_the_type_members_and_a_geo_page_keeps_entities_driving() {
        let mut query = typed_query();
        query.selection.geo = Some(within_point());
        let walk = EntityRelations::for_geo_walk(&query, &types(), None);
        assert_eq!(walk.search, Demand::NotJoined);
        assert!(matches!(
            walk.source,
            RowSource::TypeMembers {
                entities: Demand::NotJoined,
                ..
            }
        ));
        assert_eq!(relations(&query), joined(Demand::Joined));
    }

    #[test]
    fn a_walk_without_a_search_filter_joins_no_search_row() {
        for query in [empty_query(), typed_query()] {
            let relations = relations(&query);
            assert_eq!(relations.search, Demand::NotJoined, "{query:?}");
            assert_eq!(relations.walked(&query), relations, "{query:?}");
        }
    }

    /// `attrs`, a projection and `scopeQ` test each Entity the walk yields on
    /// a search row of their own, so a walk whose selectors and sort keys read
    /// none joins none, and a walk of `entities` by type keeps it per Entity.
    #[test]
    fn a_walk_beside_filters_alone_joins_no_search_row() {
        let speed = || vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        let mut by_type = typed_query();
        by_type.selection.attrs = speed();
        let mut untyped = empty_query();
        untyped.selection.attrs = speed();
        let mut scoped = typed_query();
        scoped.selection.scope_q = Some("/a".parse::<ScopeQuery>().expect("valid scope query"));
        for query in [by_type, untyped, scoped] {
            assert_eq!(relations(&query).search, Demand::Joined, "{query:?}");
            assert_eq!(relations(&query).walked(&query).search, Demand::NotJoined, "{query:?}");
        }
        let mut typed_by_identifier = typed_query();
        typed_by_identifier.selection.selectors[0].ids = vec!["urn:a".parse().expect("valid URI")];
        typed_by_identifier.selection.attrs = speed();
        assert_eq!(relations(&typed_by_identifier).walked(&typed_by_identifier).search, Demand::JoinedPerRow);
    }
}
