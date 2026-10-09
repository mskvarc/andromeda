//! Which relations an entity query statement reads, and which one drives it.
//!
//! A statement is driven by `entities`, which carries the identifier the
//! selectors filter and the ordering breaks ties on, unless the request selects
//! by type alone: then it is driven by the type-membership walk of that
//! selection (clause 4.17), which carries the same `entity_key` and `id` and
//! yields a type's Entities in identifier order without visiting any other.
//! `entities` is then joined only for the `createdAt` or `modifiedAt` a sort
//! reads. `entity_search` is joined only when some axis of the query reads an
//! indexed column of it. A `q` expression and a sort on an attribute value read
//! `current_attributes` rows through probes and laterals correlated on the
//! entity key, so they join nothing. What the response holds is not a reason
//! to join either: the head of each Entity is looked up by key for the rows of
//! the cut page alone, and its attributes are read afterwards by the keys of
//! the page. A query that asks for identifiers with no filter joins nothing
//! and reads one relation.

use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::{ConsideredEntities, EntitySelection},
    ordering::term::{CompiledOrderTerm, OrderTarget},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::{
    dictionary::resolved_names::ResolvedNames,
    entity_row_refs::EntityRowRefs,
    type_membership::{drives, members_of_any},
};
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{DynIden, Expr, ExprTrait, Iden, IntoIden, JoinType, SelectStatement, SimpleExpr, TableRef};

/// The type-membership walk a type-driven statement reads, `selected`.
#[derive(Iden, Clone, Copy)]
#[iden = "selected"]
pub struct Selected;

/// Whether a relation takes part in the statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demand {
    /// The relation is joined to the driving row on `entity_key`.
    Joined,
    /// The relation is not named in the statement.
    NotJoined,
}

/// The relation a statement is driven by.
#[derive(Debug, Clone, PartialEq)]
pub enum RowSource {
    /// `entities`, walked or probed as the filters choose.
    Entities,
    /// The type-membership walk of the request's type selection, read under
    /// [`Selected`], with `entities` joined as `entities` says.
    TypeMembers {
        /// The walk: every Entity the selection selects, as `(entity_key, id)`.
        walk: Box<SelectStatement>,
        /// Whether `entities` is joined for a lifecycle column.
        entities: Demand,
    },
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
    pub fn for_query(query: &CompiledEntityQuery, types: &ResolvedNames<EntityTypeName>, schema: Option<&DynIden>) -> EntityRelations {
        let selection = &query.selection;
        let terms = &query.ordering.terms;
        let lifecycle = demand(terms.iter().any(reads_lifecycle));
        let source = source(selection, types, schema, lifecycle);
        let search = search_demand(selection, &source) || terms.iter().any(reads_search_row);
        EntityRelations {
            source,
            search: demand(search),
        }
    }

    /// The relations the fallback count of a paginated query reads: the same
    /// source and filters, no projection and no ordering.
    #[must_use]
    pub fn for_count(query: &CompiledEntityQuery, types: &ResolvedNames<EntityTypeName>, schema: Option<&DynIden>) -> EntityRelations {
        let selection = &query.selection;
        let source = source(selection, types, schema, Demand::NotJoined);
        EntityRelations {
            search: demand(search_demand(selection, &source)),
            source,
        }
    }

    /// How the statement's fragments reach the rows, in the given branch
    /// schema, with the key and identifier read from the membership walk when
    /// it drives the statement.
    #[must_use]
    pub fn rows(&self, schema: Option<DynIden>) -> EntityRowRefs {
        let rows = EntityRowRefs::unaliased(schema);
        match self.source {
            RowSource::Entities => rows,
            RowSource::TypeMembers { .. } => rows.keyed_by(Selected),
        }
    }

    /// Add the driving relation and the joins this demand asks for to
    /// `select`, every relation qualified by the branch schema (`None` for the
    /// live branch).
    pub fn apply_from(self, select: &mut SelectStatement, schema: Option<&DynIden>) {
        let driving_key: SimpleExpr = match self.source {
            RowSource::Entities => {
                select.from(qualified(schema, Entities::Table));
                Expr::col((Entities::Table, Entities::EntityKey))
            }
            RowSource::TypeMembers { walk, entities } => {
                select.from_subquery(*walk, Selected);
                let key = Expr::col((Selected, Entities::EntityKey));
                join_on_key(
                    select,
                    entities,
                    qualified(schema, Entities::Table),
                    (Entities::Table, Entities::EntityKey),
                    &key,
                );
                key
            }
        };
        join_on_key(
            select,
            self.search,
            qualified(schema, EntitySearch::Table),
            (EntitySearch::Table, EntitySearch::EntityKey),
            &driving_key,
        );
    }
}

/// The relation a selection is driven by: the membership walk when every
/// selector selects by a type expression that has a range to walk and names no
/// identifier, `entities` otherwise.
///
/// A selector naming identifiers is already answered by the identifier index,
/// and a request that considers no Entity (clause 5.5.14) walks nothing.
fn source(selection: &EntitySelection, types: &ResolvedNames<EntityTypeName>, schema: Option<&DynIden>, lifecycle: Demand) -> RowSource {
    let walkable = match selection.considered {
        ConsideredEntities::Selected => {
            !selection.selectors.is_empty()
                && selection
                    .selectors
                    .iter()
                    .all(|selector| selector.ids.is_empty() && selector.type_expr.as_ref().is_some_and(drives))
        }
        ConsideredEntities::NoEntity => false,
    };
    let walk = walkable
        .then(|| members_of_any(selection.selectors.iter().filter_map(|selector| selector.type_expr.as_ref()), types, schema))
        .flatten();
    match walk {
        Some(walk) => RowSource::TypeMembers {
            walk: Box::new(walk),
            entities: lifecycle,
        },
        None => RowSource::Entities,
    }
}

/// Whether a selection's filters read the search row: a type selector the
/// membership walk does not answer, or any other search axis.
fn search_demand(selection: &EntitySelection, source: &RowSource) -> bool {
    let typed = match source {
        RowSource::Entities => selection.selectors.iter().any(|selector| selector.type_expr.is_some()),
        RowSource::TypeMembers { .. } => false,
    };
    typed || !selection.attrs.is_empty() || selection.projected_members.constrains() || selection.geo.is_some() || selection.scope_q.is_some()
}

/// Join `table` on its `entity_key` column `key` to the driving `driving_key`
/// when `demand` asks for it.
fn join_on_key(select: &mut SelectStatement, demand: Demand, table: TableRef, key: (impl IntoIden, impl IntoIden), driving_key: &SimpleExpr) {
    match demand {
        Demand::Joined => {
            select.join(JoinType::Join, table, Expr::col(key).eq(driving_key.clone())); // clone: every join correlates on the same key
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
const fn reads_lifecycle(term: &CompiledOrderTerm) -> bool {
    match &term.target {
        OrderTarget::CreatedAt | OrderTarget::ModifiedAt => true,
        OrderTarget::Id | OrderTarget::Type | OrderTarget::Scope | OrderTarget::Attribute(_) => false,
    }
}

/// Whether an ordering term reads a column of the search row.
fn reads_search_row(term: &CompiledOrderTerm) -> bool {
    match &term.target {
        OrderTarget::Type | OrderTarget::Scope => true,
        // Only a term naming the attribute itself can read the dedicated
        // geometry column; a path descending into it reads the attribute's rows.
        OrderTarget::Attribute(path) => term.direction.is_distance() && path.is_simple() && path.attr.as_str() == LOCATION.as_str(),
        // `createdAt` and `modifiedAt` read the identity row.
        OrderTarget::Id | OrderTarget::CreatedAt | OrderTarget::ModifiedAt => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_relations::{Demand, EntityRelations, RowSource};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::{
        entity::compiled::CompiledEntityQuery,
        entity_selection::{ConsideredEntities, EntitySelection, EntitySelector},
        filter::{parser, path::AttrPath},
        geo::query::GeoQuery,
        ordering::term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        pattern::Pattern,
        scope::query::ScopeQuery,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_query::{dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};
    use andromeda_postgresql_schema::current::Entities;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn empty_query() -> CompiledEntityQuery {
        CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build()
    }

    fn term(target: OrderTarget, direction: OrderDirection) -> CompiledOrderTerm {
        CompiledOrderTerm {
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
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(IriBuf::new("https://example.com/T".to_string()).expect("valid IRI"))))
                .build(),
        ];
        query
    }

    /// The FROM clause and joins `relations` renders, selecting the identifier
    /// the rows it keys read.
    fn from_clause(relations: EntityRelations) -> String {
        let mut select = Query::select();
        select.expr(relations.rows(None).identity_column(Entities::Id));
        relations.apply_from(&mut select, None);
        select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_identifier_query_with_no_filter_joins_nothing() {
        assert_eq!(relations(&empty_query()), joined(Demand::NotJoined));
        let sql = from_clause(relations(&empty_query()));
        assert_eq!(sql, r#"SELECT "entities"."id" FROM "entities""#);
    }

    /// A `q` filter reads the rows of its attributes through probes on the
    /// entity key, so it joins nothing.
    #[test]
    fn a_q_filter_joins_nothing() {
        let mut filtered = empty_query();
        filtered.selection.q_expr = Some(parser::parse("\"https://example.com/speed\">1").expect("valid q"));
        assert_eq!(from_clause(relations(&filtered)), r#"SELECT "entities"."id" FROM "entities""#);
    }

    #[test]
    fn every_search_axis_joins_the_search_row() {
        let mut attrs = empty_query();
        attrs.selection.attrs = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        let mut geo = empty_query();
        geo.selection.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point"),
            geoproperty: LOCATION.as_str().parse().expect("valid attribute name"),
        });
        let mut scope = empty_query();
        scope.selection.scope_q = Some(ScopeQuery::parse("/a").expect("valid scope query"));
        let mut by_type = empty_query();
        by_type.ordering.terms = vec![term(OrderTarget::Type, OrderDirection::Asc)];
        let mut by_distance = empty_query();
        by_distance.ordering.terms = vec![term(
            OrderTarget::Attribute(AttrPath::simple(LOCATION.as_str().parse().expect("valid attribute name"))),
            OrderDirection::DistAsc,
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
        assert_eq!(
            from_clause(relations),
            r#"SELECT "selected"."id" FROM (SELECT "entity_key", "id" FROM "entity_type_members" WHERE "entity_type_name_key" = (4)) AS "selected""#
        );
    }

    /// An identifier pattern is a filter on the walk's identifier and keeps the
    /// walk; the wildcard, and a request that considers no Entity, walk nothing.
    #[test]
    fn a_pattern_keeps_the_walk_and_the_wildcard_or_no_entity_does_not() {
        let mut patterned = typed_query();
        patterned.selection.selectors[0].id_pattern = Some(Pattern::parse("urn:a.*").expect("valid pattern"));
        assert!(matches!(relations(&patterned).source, RowSource::TypeMembers { .. }));

        let mut wildcard = empty_query();
        wildcard.selection.selectors = vec![EntitySelector::builder().type_expr(Some(TypeExpr::Wildcard)).build()];
        assert_eq!(relations(&wildcard).source, RowSource::Entities);

        let mut nothing = typed_query();
        nothing.selection.considered = ConsideredEntities::NoEntity;
        assert_eq!(relations(&nothing).source, RowSource::Entities);
    }

    /// A `createdAt` sort reads a lifecycle column the walk does not carry, so
    /// `entities` is joined to the walk on its key.
    #[test]
    fn a_created_at_sort_joins_the_entities_row_to_the_walk() {
        let mut query = typed_query();
        query.ordering.terms = vec![term(OrderTarget::CreatedAt, OrderDirection::Asc)];
        let relations = relations(&query);
        assert!(matches!(relations.source, RowSource::TypeMembers { entities: Demand::Joined, .. }));
        assert!(from_clause(relations).ends_with(r#"AS "selected" JOIN "entities" ON "entities"."entity_key" = "selected"."entity_key""#));
    }

    /// Every relation a walked statement joins correlates on the walk's key.
    #[test]
    fn a_walked_statement_joins_on_the_walks_key() {
        let mut query = typed_query();
        query.selection.q_expr = Some(parser::parse("\"https://example.com/speed\">1").expect("valid q"));
        query.selection.attrs = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        let sql = from_clause(relations(&query));
        assert!(
            sql.contains(r#"JOIN "entity_search" ON "entity_search"."entity_key" = "selected"."entity_key""#),
            "{sql}"
        );
        assert!(!sql.contains("entity_documents"), "{sql}");
    }

    /// An attribute value ordering reads the attribute's rows through a
    /// lateral on the entity key, so it joins nothing.
    #[test]
    fn an_attribute_ordering_joins_nothing() {
        let mut ordered = empty_query();
        ordered.ordering.terms = vec![term(
            OrderTarget::Attribute(AttrPath::simple("https://example.com/speed".parse().expect("valid attribute name"))),
            OrderDirection::Asc,
        )];
        assert_eq!(relations(&ordered), joined(Demand::NotJoined));
    }

    /// `createdAt` and `modifiedAt` are columns of the identity row, so an
    /// ordering on either joins nothing beside it.
    #[test]
    fn a_system_time_ordering_joins_nothing() {
        for target in [OrderTarget::ModifiedAt, OrderTarget::CreatedAt] {
            let mut query = empty_query();
            query.ordering.terms = vec![term(target, OrderDirection::Desc)];
            assert_eq!(relations(&query), joined(Demand::NotJoined));
        }
    }

    /// A `modifiedAt` sort reads the identity row, which a type-driven walk
    /// joins on its key, as it does for `createdAt`.
    #[test]
    fn a_modified_at_sort_joins_the_entities_row_to_the_walk() {
        let mut query = typed_query();
        query.ordering.terms = vec![term(OrderTarget::ModifiedAt, OrderDirection::Asc)];
        assert!(matches!(relations(&query).source, RowSource::TypeMembers { entities: Demand::Joined, .. }));
    }

    #[test]
    fn a_selector_on_identifiers_alone_needs_no_search_row() {
        let mut query = empty_query();
        query.selection.selectors = vec![EntitySelector::builder().ids(vec!["urn:a".parse().expect("valid URI")]).build()];
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
        query.ordering.terms = vec![term(OrderTarget::CreatedAt, OrderDirection::Asc)];
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
        let sql = from_clause(joined(Demand::Joined));
        assert!(
            sql.contains(r#"FROM "entities" JOIN "entity_search" ON "entity_search"."entity_key" = "entities"."entity_key""#),
            "{sql}"
        );
    }
}
