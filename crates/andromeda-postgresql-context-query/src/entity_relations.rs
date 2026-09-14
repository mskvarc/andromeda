//! Which of an entity's three current-state relations a query statement has
//! to join.
//!
//! `entities` is always the driving relation: it carries the identifier the
//! selectors filter and the ordering breaks ties on. `entity_search` is joined
//! only when some axis of the query reads an indexed column of it, and
//! `entity_documents` only when a filter or an ordering reads the payload — a
//! `q` expression, or a sort on an attribute value. A projected payload is not
//! a reason to join: it is read by a scalar lookup on the identity row's key,
//! which the planner evaluates after the sort and the limit, so a page fetches
//! only its own documents. A query that asks for identifiers with no filter
//! joins neither and reads one relation.

use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    ordering::term::{CompiledOrderTerm, OrderTarget},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_query::entity_row_refs::{DocumentsRef, EntityRowRefs};
use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{DynIden, Expr, ExprTrait, JoinType, SelectStatement};

/// Whether a relation takes part in the statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demand {
    /// The relation is joined to the driving `entities` row on `entity_key`.
    Joined,
    /// The relation is not named in the statement.
    NotJoined,
}

/// The join demands of one entity statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityRelations {
    /// Whether `entity_documents` is joined.
    pub documents: Demand,
    /// Whether `entity_search` is joined.
    pub search: Demand,
}

impl EntityRelations {
    /// The relations a compiled query needs beside `entities`.
    ///
    /// The search row is joined for a type selector, an `attrs` term, a geo
    /// query, a scope query, an ordering on `type` or `scope`, or a distance
    /// ordering measured to the default `location`. The document row is joined
    /// when a `q` expression is present (its jsonpath fallback reads the
    /// payload) or when an ordering sorts on an attribute value; a projected
    /// payload alone is read by key lookup instead.
    pub fn for_query(query: &CompiledEntityQuery) -> EntityRelations {
        let selection = &query.selection;
        let terms = &query.ordering.terms;
        let search = selection.selectors.iter().any(|selector| selector.type_expr.is_some())
            || !selection.attrs.is_empty()
            || selection.geo.is_some()
            || selection.scope_q.is_some()
            || terms.iter().any(reads_search_row);
        let documents = selection.q_expr.is_some() || terms.iter().any(reads_payload);
        EntityRelations {
            documents: demand(documents),
            search: demand(search),
        }
    }

    /// The relations the fallback count of a paginated query needs: the same
    /// filters, no projection and no ordering.
    #[must_use]
    pub fn for_count(query: &CompiledEntityQuery) -> EntityRelations {
        let selection = &query.selection;
        let search = selection.selectors.iter().any(|selector| selector.type_expr.is_some())
            || !selection.attrs.is_empty()
            || selection.geo.is_some()
            || selection.scope_q.is_some();
        EntityRelations {
            documents: demand(selection.q_expr.is_some()),
            search: demand(search),
        }
    }

    /// How the statement's fragments reach the three rows, in the given branch
    /// schema: the joined `entity_documents` column when the document row is
    /// joined, a scalar lookup by `entity_key` when it is not.
    #[must_use]
    pub fn rows(&self, schema: Option<DynIden>) -> EntityRowRefs {
        match self.documents {
            Demand::Joined => EntityRowRefs::unaliased(schema),
            Demand::NotJoined => EntityRowRefs::unaliased_with(schema, DocumentsRef::ByKeyLookup),
        }
    }

    /// Add `FROM entities` and the joins this demand asks for to `select`, every
    /// relation qualified by the branch schema (`None` for the live branch).
    pub fn apply_from(&self, select: &mut SelectStatement, schema: Option<&DynIden>) {
        select.from(qualified(schema, Entities::Table));
        match self.search {
            Demand::Joined => {
                select.join(
                    JoinType::Join,
                    qualified(schema, EntitySearch::Table),
                    Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
                );
            }
            Demand::NotJoined => {}
        }
        match self.documents {
            Demand::Joined => {
                select.join(
                    JoinType::Join,
                    qualified(schema, EntityDocuments::Table),
                    Expr::col((EntityDocuments::Table, EntityDocuments::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
                );
            }
            Demand::NotJoined => {}
        }
    }
}

/// `Joined` when `needed`.
const fn demand(needed: bool) -> Demand {
    if needed { Demand::Joined } else { Demand::NotJoined }
}

/// Whether an ordering term reads a column of the search row.
fn reads_search_row(term: &CompiledOrderTerm) -> bool {
    match &term.target {
        OrderTarget::Type | OrderTarget::Scope => true,
        // Only a term naming the attribute itself can read the dedicated
        // geometry column; a path descending into it reads the payload.
        OrderTarget::Attribute(path) => term.direction.is_distance() && path.is_simple() && path.attr.as_str() == LOCATION.as_str(),
        OrderTarget::Id => false,
    }
}

/// Whether an ordering term reads the payload: a value ordering on an
/// attribute. A distance ordering reads a geometry column or the projection.
const fn reads_payload(term: &CompiledOrderTerm) -> bool {
    match &term.target {
        OrderTarget::Attribute(_) => !term.direction.is_distance(),
        OrderTarget::Id | OrderTarget::Type | OrderTarget::Scope => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_relations::{Demand, EntityRelations};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::{
        entity::compiled::CompiledEntityQuery,
        entity_selection::{EntitySelection, EntitySelector},
        filter::{parser, path::AttrPath},
        geo::query::GeoQuery,
        ordering::term::{CompiledOrderTerm, OrderDirection, OrderTarget},
        scope::query::ScopeQuery,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
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

    fn joined(documents: Demand, search: Demand) -> EntityRelations {
        EntityRelations { documents, search }
    }

    #[test]
    fn an_identifier_query_with_no_filter_joins_nothing() {
        assert_eq!(EntityRelations::for_query(&empty_query()), joined(Demand::NotJoined, Demand::NotJoined));
        let mut select = Query::select();
        select.column(("entities", "id"));
        EntityRelations::for_query(&empty_query()).apply_from(&mut select, None);
        let sql = select.to_string(PostgresQueryBuilder);
        assert!(!sql.contains("JOIN"), "{sql}");
    }

    #[test]
    fn a_payload_query_with_no_filter_joins_nothing_and_reads_the_payload_by_key() {
        let relations = EntityRelations::for_query(&empty_query());
        assert_eq!(relations, joined(Demand::NotJoined, Demand::NotJoined));
        let sql = Query::select()
            .expr(relations.rows(None).payload_column())
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"(SELECT "payload" FROM "entity_documents" WHERE "entity_key" = "entities"."entity_key")"#),
            "{sql}"
        );
    }

    #[test]
    fn a_joined_document_row_is_read_as_its_column() {
        let mut filtered = empty_query();
        filtered.selection.q_expr = Some(parser::parse("\"https://example.com/speed\">1").expect("valid q"));
        let relations = EntityRelations::for_query(&filtered);
        let sql = Query::select()
            .expr(relations.rows(None).payload_column())
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""entity_documents"."payload""#) && !sql.contains("SELECT \"payload\" FROM"),
            "{sql}"
        );
    }

    #[test]
    fn every_search_axis_joins_the_search_row() {
        let mut typed = empty_query();
        typed.selection.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(IriBuf::new("https://example.com/T".to_string()).expect("valid IRI"))))
                .build(),
        ];
        let mut attrs = empty_query();
        attrs.selection.attrs = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        let mut geo = empty_query();
        geo.selection.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: Geometry::from_geojson_value(&serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point"),
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
        for query in [typed, attrs, geo, scope, by_type, by_distance] {
            assert_eq!(EntityRelations::for_query(&query).search, Demand::Joined, "{query:?}");
        }
    }

    #[test]
    fn a_q_filter_and_an_attribute_ordering_join_the_documents() {
        let mut filtered = empty_query();
        filtered.selection.q_expr = Some(parser::parse("\"https://example.com/speed\">1").expect("valid q"));
        assert_eq!(EntityRelations::for_query(&filtered).documents, Demand::Joined);
        let mut ordered = empty_query();
        ordered.ordering.terms = vec![term(
            OrderTarget::Attribute(AttrPath::simple("https://example.com/speed".parse().expect("valid attribute name"))),
            OrderDirection::Asc,
        )];
        assert_eq!(EntityRelations::for_query(&ordered).documents, Demand::Joined);
    }

    #[test]
    fn a_selector_on_identifiers_alone_needs_no_search_row() {
        let mut query = empty_query();
        query.selection.selectors = vec![EntitySelector::builder().ids(vec!["urn:a".parse().expect("valid URI")]).build()];
        assert_eq!(EntityRelations::for_query(&query).search, Demand::NotJoined);
    }

    #[test]
    fn the_count_joins_what_its_filters_read_and_never_the_projection() {
        let mut query = empty_query();
        query.selection.attrs = vec![IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")];
        assert_eq!(EntityRelations::for_count(&query), joined(Demand::NotJoined, Demand::Joined));
    }

    #[test]
    fn the_joins_are_on_the_surrogate_key() {
        let mut select = Query::select();
        select.column(("entities", "id"));
        joined(Demand::Joined, Demand::Joined).apply_from(&mut select, None);
        let sql = select.to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "entities" JOIN "entity_search" ON "entity_search"."entity_key" = "entities"."entity_key" JOIN "entity_documents" ON "entity_documents"."entity_key" = "entities"."entity_key""#), "{sql}");
    }
}
