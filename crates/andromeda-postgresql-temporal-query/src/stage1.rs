//! Stage 1 of a temporal query: select the entity ids that match every filter.
//!
//! The statement drives from the temporal catalogue, never from `entities`.
//! Core Entity deletion (clause 5.6.6) removes the current-state row and leaves
//! the temporal evolution standing, so an entity's history has to stay
//! selectable after it; the entity URI and the entity types are carried on the
//! temporal catalogue for exactly that reason. The value filters read the
//! temporal evolution too: clause 5.7.4.4 checks `q` (step S2) and the geoquery
//! (step S3) against the Attribute instances inside the temporal window, which
//! are the history rows, never against current state. On the live branch every
//! one of those relations is the history source that also holds the entries
//! still queued, so a write is selected the moment it is acknowledged. A
//! selection by type alone walks the type memberships of the temporal
//! catalogue instead, so a page of a type visits that type's Evolutions in
//! identifier order and no other, and a selection by attribute with no type
//! walks the streams of its attributes the same way.
//!
//! The `datasetId` parameter selects no Entity. Clause 5.7.4.4 applies it at
//! step S8, after every selection step, to "filter the Attribute instances" and
//! "[r]emove all Attributes without remaining Attribute instances"; only step
//! S1 removes an Entity, and only for having no data in the window. So neither
//! the window match, nor `q`, nor the geoquery narrows by dataset here: an
//! Entity whose instances the parameter all filters out is selected, counted
//! and paged, and answers with no Attribute. The one criterion that reads the
//! parameter is a `pick` / `omit` projection's (`crate::projected_members`):
//! clause 5.7.4.5 reduces the Entity after step S8, so whether the projection
//! leaves it a member depends on the instances S8 kept.

use crate::{
    alias::{Ah, EntityIdColumn, S},
    axis::axis_column,
    projected_members::projected_member_condition,
    value_filters::value_filter_condition,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity_selection::{ConsideredEntities, EntitySelector},
    identifier_axis::IdentifierAxis,
    page_window::{PageStart, PageWindow},
    read_order::ReadOrder,
    temporal::compiled::CompiledTemporalQuery,
    type_selection::grammar::TypeExpr,
};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    entity_ordering::{entity_identifier_key, ordering_collation},
    error::RenderError,
    history_source::{
        HistoryBranch,
        attribute_rows::{attribute_rows, stream_rows},
        entity_rows::{attribute_holder_rows, entity_rows, type_member_rows},
        grain::HistoryGrain,
    },
    page_window::apply_identifier_window,
    time_interval_predicate::within_interval,
    type_membership::drives,
    type_predicate::condition as type_condition,
};
use andromeda_postgresql_schema::{current::E, temporal::TemporalEntities};
use andromeda_postgresql_seaquery::{
    literal::{false_literal, int_literal},
    operator::regex_match,
};
use sea_query::{Condition, DynIden, Expr as SeaExpr, ExprTrait, Func, IntoIden, SelectStatement, TableRef};

/// The Stage 1 statement and its optional count, each a statement tree the store
/// binds and executes on its own.
///
/// The result and the count carry the same entity predicate but different tails
/// (the result paginates and orders, the count neither), so each owns its own
/// placeholder sequence rather than sharing one, which is what a paginated
/// result and an unpaginated count cannot do.
pub struct Stage1Query {
    /// The paginated statement that selects the matching entity ids.
    pub select: SelectStatement,
    /// The unpaginated `COUNT`, present when clause 5.5.9 asks for a total.
    pub count: Option<SelectStatement>,
    /// The order the page's identifiers come back in, which the store presents
    /// ascending.
    pub read_order: ReadOrder,
}

/// Build Stage 1 query: find the identifiers `window` selects of the entities
/// matching entity-level filters, counting every match when `count` asks for
/// it.
///
/// # Errors
///
/// Returns [`RenderError::Query`] when the current-state filters cannot be
/// rendered: a `q` term (clause 4.9) naming an operator the store cannot apply
/// to the target path, or a geo query (clause 4.10) whose relation and geometry
/// combination clause 4.10 does not admit. Returns
/// [`RenderError::ReferenceGeometryEncoding`] when the geo query's reference
/// geometry cannot be encoded as `PostGIS` EWKB.
pub fn build_stage1_entity_ids(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    window: &PageWindow,
    count: CountPolicy,
    names: &QueryNames,
) -> Result<Stage1Query, RenderError> {
    let (evolutions, type_test) = evolution_rows(branch, compiled, names);
    // Entity-level conditions, applied to the Evolutions under alias "e".
    let entity_conditions = build_entity_conditions_for_temporal(branch, compiled, names, type_test)?;

    let mut select = SelectStatement::new();
    select
        .expr_as(SeaExpr::col((E, TemporalEntities::Id)), EntityIdColumn)
        .from(evolutions.clone()) // clone: the count query reads the same Evolutions
        .cond_where(entity_conditions.clone()) // clone: the count query applies the same entity predicate
        .cond_where(SeaExpr::exists(temporal_match_subquery(branch, compiled, names)));

    // Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    // the JSON-LD array returned shall be ordered according to the member names
    // specified." Only `id` can be named (clause 5.7.4.3), and the collation is
    // written out rather than left to the column: `temporal_entities.id` is
    // declared `COLLATE "C"`, so an unstated comparison is byte-wise and the
    // collation Table 6.18.3.2-1 says the payload "shall be ordered according
    // to" would have no effect at all.
    //
    // Clause 5.5.9 asks for a consistent order across pages whether or not one
    // was requested, so the `id` follows as the tiebreaker of an offset page.
    // A cursor page (clause 5.5.9.1) walks the element dimension's default
    // `id` order alone: the pagination policy never pairs a cursor with a
    // requested ordering, which its identifier position cannot walk. The
    // window governs only the element dimension, never the temporal window
    // (`Content-Range`).
    match (&window.start, compiled.ordering.as_ref()) {
        (PageStart::Offset(_), Some(ordering)) => {
            entity_identifier_key(
                SeaExpr::col((E, TemporalEntities::Id)),
                ordering.direction,
                &ordering_collation(ordering.collation.as_ref()),
            )
            .order_by(&mut select);
        }
        (PageStart::Offset(_), None) | (PageStart::Cursor(_), _) => {}
    }
    let read_order = apply_identifier_window(&mut select, SeaExpr::col((E, TemporalEntities::Id)), window);

    let count_statement = match count {
        CountPolicy::Emit => {
            let mut count_select = SelectStatement::new();
            count_select
                .expr(Func::count(SeaExpr::col((E, TemporalEntities::Id))))
                .from(evolutions)
                .cond_where(entity_conditions)
                .cond_where(SeaExpr::exists(temporal_match_subquery(branch, compiled, names)));
            Some(count_select.take())
        }
        CountPolicy::Omit => None,
    };

    Ok(Stage1Query {
        select: select.take(),
        count: count_statement,
        read_order,
    })
}

/// Build the correlated stream or history predicate shared by result and
/// count: a bounded window asks for an instance inside it, an unbounded one
/// only for a stream of a concerned attribute.
///
/// Over a source without queued entries the Evolution is compared beside the
/// source, so a window few Evolutions answer, or none, is found by a
/// semi-join over its rows instead of one probe per candidate.
fn temporal_match_subquery(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames) -> SelectStatement {
    let read = branch.entity_history(SeaExpr::col((E, TemporalEntities::Id)));
    let (source, alias) = if compiled.window.interval.is_bounded() {
        (attribute_rows(branch, &read.of, Ah), Ah.into_iden())
    } else {
        (stream_rows(branch, &read.of, S), S.into_iden())
    };
    let mut select = SelectStatement::new();
    select
        .expr(int_literal(1))
        .from(source)
        .cond_where(build_temporal_stage1_conditions(compiled, &alias, &names.attributes));
    if let Some(entity) = read.beside {
        select.and_where(SeaExpr::col((alias, HistoryGrain::EntityId)).eq(entity));
    }
    select.take()
}

/// How a selector's type is tested on the Evolutions stage 1 reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeTest {
    /// A containment test on the Evolution's type keys.
    TypeKeys,
    /// Nothing: the membership walk the Evolutions come from selected the
    /// type already.
    Walked,
}

/// The Evolutions stage 1 reads under `e`, and how a selector's type is
/// tested on them.
///
/// The walk of the type memberships when it answers every selector's type on
/// its own: each selector names no identifier (the identifier index answers
/// one already) and a type with a range to walk, and either there is one
/// selector or none carries a pattern. The walk yields the union of the
/// selectors' types, so it cannot tell which pattern goes with which type,
/// and an Evolution only queued has no membership row to probe for it.
/// Otherwise, when the request names attributes and no selector names a type
/// or an identifier, the walk of those attributes' streams: step S1 of clause
/// 5.7.4.4 keeps only Evolutions with an instance of one of them, which hold
/// a stream of it, and no type is tested on what it yields. A request that
/// considers no Entity (clause 5.5.14) walks nothing. Otherwise the temporal
/// catalogue, its types tested on its type keys.
fn evolution_rows(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames) -> (TableRef, TypeTest) {
    let selection = &compiled.entities;
    let selectors = &selection.selectors;
    match selection.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => return (entity_rows(branch, E), TypeTest::TypeKeys),
    }
    let type_walkable = !selectors.is_empty()
        && selectors.iter().all(|selector| selector.ids.is_empty())
        && (selectors.len() == 1 || selectors.iter().all(|selector| selector.id_pattern.is_none()));
    let exprs: Option<Vec<&TypeExpr>> = type_walkable
        .then(|| {
            selectors
                .iter()
                .map(|selector| selector.type_expr.as_ref().filter(|expr| drives(expr)))
                .collect()
        })
        .flatten();
    if let Some(walk) = exprs.and_then(|exprs| type_member_rows(branch, E, &exprs, &names.types)) {
        return (walk, TypeTest::Walked);
    }
    let holder_walkable =
        !compiled.instances.attr_filter.is_empty() && selectors.iter().all(|selector| selector.ids.is_empty() && selector.type_expr.is_none());
    if holder_walkable {
        return (
            attribute_holder_rows(branch, E, &concerned_attribute_keys(compiled, &names.attributes)),
            TypeTest::TypeKeys,
        );
    }
    (entity_rows(branch, E), TypeTest::TypeKeys)
}

/// The dictionary keys of the attributes the request concerns (clause
/// 5.7.4.4 step S1); an attribute the dictionary never keyed has no history
/// and no key.
fn concerned_attribute_keys(compiled: &CompiledTemporalQuery, attributes: &ResolvedNames<AttributeName>) -> Vec<AttributeNameKey> {
    compiled
        .instances
        .attr_filter
        .iter()
        .filter_map(|name| attributes.get_str(name.as_str()))
        .collect()
}

/// Build entity-level conditions for temporal queries, over the Evolutions
/// under the `e` alias.
fn build_entity_conditions_for_temporal(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    type_test: TypeTest,
) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // Entity selectors, unioned (clause 5.2.33): `(sel1) OR (sel2) OR ...`,
    // each reached through the `e` alias stage 1 selects the Evolutions
    // under. Rendered only when every selector constrains, the reading
    // `EntitySelection::selectors_constrain` shares with the entity path, and
    // a walk that selected the types leaves only a lone selector's pattern.
    let selectors = &compiled.entities.selectors;
    let walked_without_pattern = type_test == TypeTest::Walked && selectors.iter().all(|selector| selector.id_pattern.is_none());
    if compiled.entities.selectors_constrain() && !walked_without_pattern {
        let mut union = Condition::any();
        for selector in selectors {
            union = union.add(temporal_selector_condition(selector, &names.types, type_test));
        }
        condition = condition.add(union);
    }

    // Clause 5.5.14: "An EntityMap fixes the Entities to be considered." A
    // request narrowed to no entity at all cannot say so through a selector's
    // identifier list, where an empty list reads as no identifier constraint,
    // so it is conjoined here, where nothing can widen it back.
    match compiled.entities.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => condition = condition.add(false_literal()),
    }

    // Clause 5.7.4.4 S5 to S7: the value, geo and scope filters, from which
    // the entities a Context Source contributed to a split query are exempt.
    if let Some(filters) = value_filter_condition(branch, compiled, names)? {
        condition = condition.add(filters);
    }

    // Clauses 5.7.4.5 and 6.3.13: an entity the `pick` / `omit` projection
    // would reduce to no member is not selected, so it is left out here, where
    // the count and the page are taken.
    if let Some(members) = projected_member_condition(branch, compiled) {
        condition = condition.add(members);
    }

    Ok(condition)
}

/// Render one selector's identifier, pattern and type predicates against the
/// `e` alias stage 1 selects the temporal catalogue under, `ANDed` (clause
/// 5.2.33).
///
/// The entity path renders the same three axes against `entities`; the columns
/// carry the same names on both relations, so the two cannot drift on what a
/// selector means.
fn temporal_selector_condition(selector: &EntitySelector, types: &ResolvedNames<EntityTypeName>, type_test: TypeTest) -> Condition {
    let mut condition = Condition::all();
    match selector.identifier_axis() {
        IdentifierAxis::Ids(ids) => {
            let ids: Vec<&str> = ids.iter().map(AsRef::as_ref).collect();
            condition = condition.add(SeaExpr::col((E, TemporalEntities::Id)).is_in(ids));
        }
        IdentifierAxis::Pattern(pattern) => {
            condition = condition.add(regex_match(SeaExpr::col((E, TemporalEntities::Id)), SeaExpr::val(pattern.portable().as_str())));
        }
        IdentifierAxis::Any => {}
    }
    match (&selector.type_expr, type_test) {
        (Some(type_expr), TypeTest::TypeKeys) => {
            condition = condition.add(type_condition(type_expr, &SeaExpr::col((E, TemporalEntities::EntityTypeNameKeys)), types));
        }
        (Some(_), TypeTest::Walked) | (None, TypeTest::TypeKeys | TypeTest::Walked) => {}
    }
    condition
}

/// Build the clause 5.7.4.4 step S1 conditions of Stage 1 over the source
/// under `alias`: the concerned attributes and the window.
///
/// The attributes are compared by dictionary key on the source's own grain
/// column, which PostgreSQL pushes into every branch of the source, so the
/// queued entries of other attributes are dropped before the rules that
/// judge whether an entry is written; an attribute the dictionary never
/// keyed has no history and matches nothing.
fn build_temporal_stage1_conditions(compiled: &CompiledTemporalQuery, alias: &DynIden, attributes: &ResolvedNames<AttributeName>) -> Condition {
    let mut condition = Condition::all();

    if !compiled.instances.attr_filter.is_empty() {
        let keys: Vec<i32> = concerned_attribute_keys(compiled, attributes).into_iter().map(AttributeNameKey::get).collect();
        // clone: the alias is shared by every condition on the source.
        condition = condition.add(SeaExpr::col((alias.clone(), HistoryGrain::AttributeNameKey)).is_in(keys));
    }

    // Temporal filters (timerel + time_at), on the requested axis alone.
    let axis = axis_column(compiled.window.timeproperty);
    condition.add(within_interval(&SeaExpr::col((Ah, axis)), &compiled.window.interval))
}

#[cfg(test)]
mod tests {
    use crate::stage1::build_stage1_entity_ids;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        dataset::DatasetSelector,
        entity_selection::{EntitySelection, EntitySelector},
        filter::parser,
        geo::query::GeoQuery,
        ordering::{collation::IcuCollation, sort_direction::SortDirection},
        page_window::{PageStart, PageWindow},
        pattern::Pattern,
        scope::query::ScopeQuery,
        temporal::{
            compiled::CompiledTemporalQuery,
            contributed_entities::{ContributedEntities, ContributedRendering},
            instance_selection::InstanceSelection,
            ordering::TemporalEntityOrdering,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_type_name_key::EntityTypeNameKey,
        history_source::HistoryBranch,
    };
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Value, Values};
    use std::str::FromStr;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    fn ids(raw: &[&str]) -> Vec<Identifier> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// The type dictionary with the one type the selector tests name.
    fn vehicle_is_known() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::default(),
            types: ResolvedNames::from_pairs([(
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".parse().expect("valid type name"),
                EntityTypeNameKey::from(3),
            )]),
        }
    }

    /// A `GeoJSON` point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: &serde_json::Value) -> Geometry {
        serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
    }

    /// A query with every filter absent, for tests that vary one part at a time.
    fn minimal_compiled() -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    /// The Stage 1 result statement, rendered with `$N` placeholders and its
    /// bound values, which is what the store hands to the driver.
    fn built(compiled: &CompiledTemporalQuery, names: &QueryNames) -> (String, Values) {
        built_windowed(compiled, &PageWindow::offset(20, 0), names)
    }

    /// The Stage 1 result statement of the page `window` selects.
    fn built_windowed(compiled: &CompiledTemporalQuery, window: &PageWindow, names: &QueryNames) -> (String, Values) {
        build_stage1_entity_ids(&HistoryBranch::live(), compiled, window, CountPolicy::Omit, names)
            .expect("valid temporal query")
            .select
            .build(PostgresQueryBuilder)
    }

    /// The count statement of a counted query over the page `window` selects.
    fn built_windowed_count(compiled: &CompiledTemporalQuery, window: &PageWindow, names: &QueryNames) -> String {
        build_stage1_entity_ids(&HistoryBranch::live(), compiled, window, CountPolicy::Emit, names)
            .expect("valid temporal query")
            .count
            .expect("count requested")
            .build(PostgresQueryBuilder)
            .0
    }

    /// The count statement of a counted query.
    fn built_count(compiled: &CompiledTemporalQuery, names: &QueryNames) -> String {
        build_stage1_entity_ids(&HistoryBranch::live(), compiled, &PageWindow::offset(20, 0), CountPolicy::Emit, names)
            .expect("valid temporal query")
            .count
            .expect("count requested")
            .build(PostgresQueryBuilder)
            .0
    }

    fn has_timestamp(values: &Values) -> bool {
        values.0.iter().any(|value| matches!(value, Value::ChronoDateTimeUtc(_)))
    }

    fn vehicle() -> TypeExpr {
        TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Vehicle"))
    }

    fn typed(selectors: Vec<EntitySelector>) -> CompiledTemporalQuery {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = selectors;
        compiled
    }

    fn vehicle_selector() -> EntitySelector {
        EntitySelector::builder().type_expr(Some(vehicle())).build()
    }

    /// Clause 4.17: a selection by type alone walks the type's membership
    /// range, which yields its Evolutions in the clause 5.5.9 `id` order, and
    /// tests no type key array.
    #[test]
    fn a_temporal_type_selection_walks_its_type_members() {
        let compiled = typed(vec![vehicle_selector()]);
        let (sql, _) = built_windowed(&compiled, &PageWindow::offset(20, 0), &vehicle_is_known());
        assert!(sql.starts_with(r#"SELECT "e"."id" AS "entity_id" FROM"#), "{sql}");
        assert!(
            sql.contains(
                r#"FROM (SELECT "temporal_entity_key", "id" FROM "temporal_entity_type_members" WHERE "entity_type_name_key" = (3) ORDER BY "id" ASC) AS "rw""#
            ),
            "{sql}"
        );
        assert!(!sql.contains(r#""e"."entity_type_name_keys""#), "{sql}");
        assert!(!sql.contains(r#"FROM "temporal_entities" AS "re" UNION ALL"#), "{sql}");
        assert!(sql.contains(r#") AS "s" WHERE"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" ASC"#), "{sql}");
    }

    /// Clause 4.17 `,`: the walk merges the ranges of both types, each
    /// Evolution once, in identifier order.
    #[test]
    fn a_type_disjunction_walks_the_merged_ranges_in_identifier_order() {
        let bus = TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Bus"));
        let mut names = vehicle_is_known();
        names.types = ResolvedNames::from_pairs([
            (
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".parse().expect("valid type name"),
                EntityTypeNameKey::from(3),
            ),
            (
                "https://uri.etsi.org/ngsi-ld/default-context/Bus".parse().expect("valid type name"),
                EntityTypeNameKey::from(4),
            ),
        ]);
        let compiled = typed(vec![EntitySelector::builder().type_expr(Some(TypeExpr::Or(vec![vehicle(), bus]))).build()]);
        let (sql, _) = built(&compiled, &names);
        assert!(
            sql.contains(r#"FROM (SELECT DISTINCT ON ("typed"."id") "typed"."temporal_entity_key", "typed"."id" FROM ("#),
            "{sql}"
        );
        assert!(sql.contains(r#"ORDER BY "typed"."id" ASC, "id" ASC) AS "rw""#), "{sql}");
        assert!(
            sql.contains(r#""rq"."entity_type_name_keys" @> (ARRAY[3])) OR ("rq"."entity_type_name_keys" @> (ARRAY[4]))"#),
            "{sql}"
        );
    }

    /// A Snapshot has no queue, so its type walk is the membership range alone.
    #[test]
    fn a_snapshot_type_selection_walks_its_own_type_members() {
        let compiled = typed(vec![vehicle_selector()]);
        let sql = build_stage1_entity_ids(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Omit,
            &vehicle_is_known(),
        )
        .expect("valid temporal query")
        .select
        .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"FROM (SELECT "temporal_entity_key", "id" FROM "snap_7"."temporal_entity_type_members" WHERE "entity_type_name_key" = (3)) AS "e""#),
            "{sql}"
        );
    }

    /// The count of a type selection counts the same walk the page cuts.
    #[test]
    fn a_counted_type_selection_counts_its_type_members() {
        let compiled = typed(vec![vehicle_selector()]);
        let count = built_count(&compiled, &vehicle_is_known());
        assert!(count.starts_with(r#"SELECT COUNT("e"."id") FROM"#), "{count}");
        assert!(
            count.contains(r#""temporal_entity_type_members" WHERE "entity_type_name_key" = (3)"#),
            "{count}"
        );
    }

    /// A lone selector's pattern still filters the walk of its type.
    #[test]
    fn a_lone_typed_selector_keeps_its_pattern_over_the_walk() {
        let compiled = typed(vec![
            EntitySelector::builder()
                .id_pattern(Some(Pattern::parse(".*:7$").expect("valid pattern")))
                .type_expr(Some(vehicle()))
                .build(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(sql.contains(r#""temporal_entity_type_members""#), "{sql}");
        assert!(sql.contains(r#""e"."id" ~ "#), "{sql}");
        assert!(!sql.contains(r#""e"."entity_type_name_keys""#), "{sql}");
    }

    /// A selector naming identifiers is answered by the identifier index, so
    /// it keeps the catalogue and tests the type on its type keys.
    #[test]
    fn a_typed_selector_naming_identifiers_keeps_the_catalogue() {
        let compiled = typed(vec![
            EntitySelector::builder()
                .ids(ids(&["urn:ngsi-ld:Vehicle:1"]))
                .type_expr(Some(vehicle()))
                .build(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("temporal_entity_type_members"), "{sql}");
        assert!(sql.contains(r#""e"."entity_type_name_keys" @> (ARRAY[3])"#), "{sql}");
    }

    /// The walk yields the union of the selectors' types, so it cannot pair a
    /// pattern with its own selector's type: such selectors keep the
    /// catalogue.
    #[test]
    fn several_selectors_pairing_types_with_patterns_keep_the_catalogue() {
        let compiled = typed(vec![
            EntitySelector::builder()
                .id_pattern(Some(Pattern::parse(".*:7$").expect("valid pattern")))
                .type_expr(Some(vehicle()))
                .build(),
            vehicle_selector(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("temporal_entity_type_members"), "{sql}");
        assert!(sql.contains(r#""e"."entity_type_name_keys" @> (ARRAY[3])"#), "{sql}");
    }

    /// A selector with no type has no range, so neither has the selection.
    #[test]
    fn a_selection_with_an_untyped_selector_keeps_the_catalogue() {
        let compiled = typed(vec![
            vehicle_selector(),
            EntitySelector::builder().id_pattern(Some(Pattern::parse(".*").expect("valid pattern"))).build(),
        ]);
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("temporal_entity_type_members"), "{sql}");
    }

    /// A query concerning `speed` alone over the window from 2024.
    fn speed_only() -> CompiledTemporalQuery {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        compiled.instances.attr_filter = vec![iri("https://example.com/speed")];
        compiled
    }

    /// Clause 5.7.4.4 step S1 keeps only the Evolutions with an instance of a
    /// concerned attribute, which hold a stream of it, so a selection by
    /// `attrs` alone walks that attribute's streams in the clause 5.5.9 `id`
    /// order and reads no other Evolution.
    #[test]
    fn a_temporal_attrs_only_selection_reads_the_attribute_streams() {
        let (sql, _) = built(&speed_only(), &speed_and_address());
        assert!(
            sql.starts_with(r#"SELECT "e"."id" AS "entity_id" FROM (SELECT DISTINCT ON ("held"."id") "held"."id" FROM (SELECT "hr"."id" FROM (SELECT "id" FROM "temporal_attribute_streams" WHERE "attribute_name_key" = (7) ORDER BY "id" ASC) AS "hr""#),
            "{sql}"
        );
        assert!(!sql.contains(r#"FROM "temporal_entities" AS "re" UNION ALL"#), "{sql}");
        assert!(sql.contains(r#") AS "ah" WHERE "ah"."attribute_name_key" IN ($"#), "{sql}");
        assert!(sql.ends_with(r#"ORDER BY "e"."id" ASC LIMIT $3 OFFSET $4"#), "{sql}");
    }

    /// The count totals the Evolutions the page walks.
    #[test]
    fn a_counted_attrs_only_selection_counts_the_attribute_walk() {
        let count = built_count(&speed_only(), &speed_and_address());
        assert!(count.starts_with(r#"SELECT COUNT("e"."id") FROM (SELECT DISTINCT ON ("held"."id")"#), "{count}");
        assert!(count.contains(r#""temporal_attribute_streams" WHERE "attribute_name_key" = (7)"#), "{count}");
    }

    /// A pattern names no identifier the index could answer, so the walk
    /// still reads the attribute's streams and the pattern filters what it
    /// yields.
    #[test]
    fn an_attrs_selection_with_an_id_pattern_walks_the_streams_and_keeps_the_pattern() {
        let mut compiled = speed_only();
        compiled.entities.selectors = vec![
            EntitySelector::builder()
                .id_pattern(Some(Pattern::parse(".*:7$").expect("valid pattern")))
                .build(),
        ];
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(
            sql.contains(r#""temporal_attribute_streams" WHERE "attribute_name_key" = (7) ORDER BY "id" ASC"#),
            "{sql}"
        );
        assert!(sql.contains(r#""e"."id" ~ "#), "{sql}");
    }

    /// A type has its own walk, and identifiers their own index, so neither
    /// selection walks the attribute's streams.
    #[test]
    fn an_attrs_selection_beside_a_type_or_identifiers_keeps_its_own_walk() {
        let mut names = speed_and_address();
        names.types = vehicle_is_known().types;
        let mut typed = speed_only();
        typed.entities.selectors = vec![vehicle_selector()];
        let (sql, _) = built(&typed, &names);
        assert!(sql.contains(r#""temporal_entity_type_members""#), "{sql}");
        assert!(!sql.contains(r#"AS "held""#), "{sql}");

        let mut identified = speed_only();
        identified.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1"])).build()];
        let (sql, _) = built(&identified, &names);
        assert!(sql.contains(r#"FROM "temporal_entities" AS "re" UNION ALL"#), "{sql}");
        assert!(!sql.contains(r#"AS "held""#), "{sql}");
    }

    /// Clause 5.6.6 leaves the temporal evolution standing when the current
    /// entity is deleted, so the driving relation is the temporal catalogue and
    /// an unfiltered query names `entities` nowhere.
    #[test]
    fn the_statement_drives_from_the_temporal_catalogue_and_never_joins_entities() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(
            sql.contains("(SELECT \"re\".\"id\", \"re\".\"entity_type_name_keys\", FALSE AS \"queued\" FROM \"temporal_entities\" AS \"re\""),
            "{sql}"
        );
        assert!(sql.contains(") AS \"e\""), "{sql}");
        assert!(sql.contains("\"temporal_attribute_streams\""), "{sql}");
        assert!(!sql.contains("\"entities\""), "{sql}");
        assert!(!sql.contains("temporal_entity_attributes"), "{sql}");
    }

    /// The stream catalogue carries the grain, so the correlation is on the
    /// surrogate entity key rather than on a repeated entity URI.
    #[test]
    fn the_stream_probe_reads_the_streams_of_the_selected_entity() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"re\".\"id\" = \"e\".\"id\""), "{sql}");
        assert!(sql.contains("\"pe\".\"entity_id\" = \"e\".\"id\""), "{sql}");
    }

    #[test]
    fn entity_id_selection_orders_by_id_for_stable_pagination() {
        // The driving temporal catalogue emits one row per id, and ordered
        // paging keeps clause 5.5.9 result pages stable.
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("ORDER BY \"e\".\"id\""), "{sql}");
    }

    #[test]
    fn stage1_projects_the_entity_uri_as_entity_id() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"e\".\"id\" AS \"entity_id\""), "{sql}");
    }

    #[test]
    fn stage1_timerel_placeholder_is_offset_after_id_filters() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let (sql, values) = built(&compiled, &QueryNames::default());

        assert!(
            !sql.contains("< $1"),
            "time placeholder must not collide with earlier id-filter placeholders: {sql}"
        );
        assert!(!sql.contains("::timestamptz"), "{sql}");
        assert!(has_timestamp(&values), "the window instant is bound: {values:?}");
    }

    /// Clause 4.11 defaults the selector to `observedAt` and clause 5.2.21
    /// admits only the four semantic axes, so the window filters the requested
    /// column and nothing stands in for a missing value on it.
    #[test]
    fn each_time_property_filters_its_own_column_with_no_fallback() {
        let expected = [
            (TimeProp::Observed, "\"ah\".\"observed_at\""),
            (TimeProp::Created, "\"ah\".\"created_at\""),
            (TimeProp::Modified, "\"ah\".\"modified_at\""),
            (TimeProp::Deleted, "\"ah\".\"deleted_at\""),
        ];
        for (prop, column) in expected {
            let mut compiled = minimal_compiled();
            compiled.window.timeproperty = prop;
            compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
            let (sql, _) = built(&compiled, &QueryNames::default());
            assert!(sql.contains(column), "{prop}: {sql}");
            assert!(!sql.contains("COALESCE"), "{prop}: {sql}");
            assert!(!sql.contains("\"ah\".\"recorded_at\""), "{prop}: {sql}");
        }
    }

    /// A bounded window is the only reason to read the hypertable, so an
    /// unbounded one stays on the catalogue.
    #[test]
    fn the_history_hypertable_is_joined_only_for_a_bounded_window() {
        let compiled = minimal_compiled();
        let (unbounded, _) = built(&compiled, &QueryNames::default());
        assert!(!unbounded.contains("attribute_history"), "{unbounded}");

        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let (bounded, _) = built(&compiled, &QueryNames::default());
        assert!(bounded.contains(r#") AS "ah" WHERE"#), "{bounded}");
    }

    #[test]
    fn stage1_count_reuses_the_result_predicate_bind_order() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let (_, values) = built(&compiled, &QueryNames::default());
        let count_sql = built_count(&compiled, &QueryNames::default());

        assert!(count_sql.contains("EXISTS(SELECT"), "{count_sql}");
        assert!(count_sql.contains("$1"), "{count_sql}");
        assert!(count_sql.contains("$2"), "{count_sql}");
        assert!(has_timestamp(&values), "the window instant is bound: {values:?}");
    }

    /// The concerned attributes are compared by dictionary key on the
    /// source's own grain column, a restriction PostgreSQL pushes into each
    /// branch of the source, so the window match never reads the name
    /// dictionary.
    #[test]
    fn the_attrs_selection_narrows_the_source_by_dictionary_key() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        compiled.instances.attr_filter = vec![iri("https://example.com/speed")];
        let (sql, values) = built(&compiled, &speed_and_address());
        assert!(!sql.contains("attribute_names"), "{sql}");
        assert!(sql.contains(r#") AS "ah" WHERE "ah"."attribute_name_key" IN ($"#), "{sql}");
        assert!(values.0.iter().any(|value| matches!(value, Value::Int(Some(7)))), "{values:?}");
    }

    /// An attribute the dictionary never keyed was never recorded, so a
    /// query naming only such attributes matches no Entity.
    #[test]
    fn an_attribute_never_keyed_matches_nothing() {
        let mut compiled = minimal_compiled();
        compiled.instances.attr_filter = vec![iri("https://example.com/unknown")];
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(sql.contains(r#") AS "s" WHERE $1 = $2 AND"#), "{sql}");
    }

    #[test]
    fn stage1_includes_scope_q_filter() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid/Gardens").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("scope_history"), "SQL should reference scope_history table: {sql}");
        assert!(sql.contains("lquery"), "SQL should contain lquery cast: {sql}");
    }

    /// Scope history hangs off the temporal entity, so the probe correlates on
    /// the surrogate key the relation carries.
    #[test]
    fn scope_history_correlates_on_the_temporal_entity_key() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"rsh\""), "{sql}");
        assert!(sql.contains("\"re\".\"id\" = \"e\".\"id\""), "{sql}");
    }

    #[test]
    fn stage1_scope_q_with_or_and_semantics() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("(/A;/B),/C").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("AND"), "SQL should contain AND for semicolon-separated scopes: {sql}");
        assert!(sql.contains("OR"), "SQL should contain OR for comma-separated scopes: {sql}");
    }

    #[test]
    fn temporal_scope_q_between() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Between {
            from: instant("2024-01-01T00:00:00Z"),
            to: instant("2024-06-01T00:00:00Z"),
        };
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(">="), "Between should bound the scope probe from below: {sql}");
        assert!(sql.contains('<'), "Between should bound the scope probe from above: {sql}");
    }

    #[test]
    fn temporal_scope_q_compound() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("(/A;/B),/C").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Before(instant("2024-06-01T00:00:00Z"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        // /A;/B -> AND, ,/C -> OR
        assert!(sql.contains("AND"), "Semicolon scopes produce AND: {sql}");
        assert!(sql.contains("OR"), "Comma scopes produce OR: {sql}");
        // Each scope term has its own EXISTS subquery among the entity
        // conditions, beside the stream catalogue EXISTS predicate rather than
        // inside it: the scope probe correlates on the temporal entity alone.
        pretty_assertions::assert_eq!(sql.matches(r#") AS "sh" WHERE"#).count(), 3, "three scope predicates: {sql}");
        let catalogue = sql.find(r#") AS "ah" WHERE"#).expect("the catalogue predicate");
        let scope = sql.find("scope_history").expect("a scope predicate");
        assert!(scope < catalogue, "the scope predicates precede the catalogue predicate: {sql}");
    }

    /// Two identifiers a Context Source contributed to a split query (clause
    /// 5.7.4.4).
    fn two_contributed() -> ContributedEntities {
        ContributedEntities::new(ids(&["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]), ContributedRendering::Local)
    }

    /// A query carrying all three of the filters clause 5.7.4.4 applies after
    /// aggregation, with an identifier selector and a bounded window.
    fn filtered_compiled() -> CompiledTemporalQuery {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:9"])).build()];
        compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">100").unwrap());
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid").expect("valid scope query"));
        compiled
    }

    /// Clause 5.7.4.4: the value, geo and scope filters of a split query apply
    /// after aggregation, so an entity a source contributed a part of is exempt
    /// from them locally -- and only from them: the identifier selector and the
    /// window still select it.
    #[test]
    fn contributed_identifiers_exempt_only_the_value_filters() {
        let mut compiled = filtered_compiled();
        compiled.contributed = two_contributed();
        let (sql, values) = built(&compiled, &speed_and_address());
        let exemption = sql.find(r#" OR "e"."id" IN ("#).expect("the contributed identifiers are exempt");
        let selector = sql.find(r#""e"."id" IN ("#).expect("the selector");
        let q = sql.find(r#"AS "qh""#).expect("the q probe");
        let geo = sql.find(r#"AS "ahg""#).expect("the geo probe");
        let scope = sql.find("scope_history").expect("the scope probe");
        let catalogue = sql.find(r#") AS "ah" WHERE"#).expect("the window match");
        assert!(selector < q && q < exemption && geo < exemption && scope < exemption, "{sql}");
        assert!(catalogue > exemption, "the window match stays outside the exemption: {sql}");
        let bound = values
            .0
            .iter()
            .filter(|value| matches!(value, Value::String(Some(text)) if text.starts_with("urn:ngsi-ld:Vehicle:")))
            .count();
        pretty_assertions::assert_eq!(bound, 3, "the selector id and the two contributed ids: {values:?}");
    }

    /// Without a contributed entity the statement is exactly the one a query
    /// that was never split renders.
    #[test]
    fn no_contributed_identifier_leaves_the_statement_as_it_was() {
        let compiled = filtered_compiled();
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(!sql.contains(r#" OR "e"."id" IN ("#), "{sql}");
        let mut contributed_elsewhere = filtered_compiled();
        contributed_elsewhere.contributed = ContributedEntities::default();
        pretty_assertions::assert_eq!(built(&contributed_elsewhere, &speed_and_address()).0, sql);
    }

    /// The names the `q` tests below resolve: `speed` at key 7, `address` at 9.
    fn speed_and_address() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs([
                ("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameKey::from(7)),
                ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameKey::from(9)),
            ]),
            types: ResolvedNames::default(),
        }
    }

    /// Clause 5.7.4.4 S2: a temporal `q` "shall be checked against all the
    /// Attribute instances resulting from the initial filtering performed by the
    /// temporal query", so each Query Term probes the in-window history rows of
    /// its attribute, correlated to the temporal entity, and never current state.
    #[test]
    fn stage1_q_filter_probes_the_in_window_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">100").unwrap());
        let (sql, _) = built(&compiled, &speed_and_address());
        for fragment in [
            r#") AS "qh" WHERE "qh"."attribute_name_key" ="#,
            r#""qh"."deleted_at" IS NULL"#,
            r#""qh"."observed_at" >="#,
            r#""qh"."value_number" >"#,
            r#""re"."id" = "e"."id""#,
            r#") AS "qs" JOIN LATERAL"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        assert!(!sql.contains("current_attributes"), "a temporal q never reads current state: {sql}");
        assert!(!sql.contains(r#""entities""#), "{sql}");
    }

    /// Clause 5.5.9: the count totals the same entities the page selects, so
    /// it carries both of the `q` probes, the instance in effect when the
    /// window opens included (clause 4.11, Annex C clause C.5.5).
    #[test]
    fn the_count_carries_the_same_two_probes() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">100").unwrap());
        let count = built_count(&compiled, &speed_and_address());
        assert!(count.contains("JOIN LATERAL"), "{count}");
        assert!(count.contains(r#""qe"."placed" <"#), "{count}");
        pretty_assertions::assert_eq!(count.matches(r#"AS "qh""#).count(), 2, "{count}");
    }

    /// The window a `q` term reads is on the requested Temporal Property.
    #[test]
    fn stage1_q_filter_reads_the_window_on_the_requested_timeproperty() {
        let mut compiled = minimal_compiled();
        compiled.window.timeproperty = TimeProp::Modified;
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\"==1").unwrap());
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(sql.contains(r#""qh"."modified_at" <"#), "{sql}");
    }

    /// Clause 5.7.4.4: the `datasetId` parameter is step S8, which filters
    /// Attribute instances after every selection step and removes no Entity.
    /// Neither the step S1 window match, nor the step S2 `q`, nor the step S3
    /// geoquery, nor the count names a dataset, for every form of the
    /// parameter.
    #[test]
    fn no_selection_step_narrows_by_the_dataset_parameter() {
        for selectors in [&["@none"][..], &["urn:ngsi-ld:Dataset:one"], &["@none", "urn:ngsi-ld:Dataset:one"]] {
            let mut compiled = minimal_compiled();
            compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
            compiled.instances.dataset_id_filter = selectors
                .iter()
                .map(|selector| DatasetSelector::parse(selector).expect("a valid dataset selector"))
                .collect();
            compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">100").unwrap());
            compiled.entities.geo = Some(GeoQuery {
                georel: GeoRel::Within,
                geometry: point(&serde_json::json!([1.0, 2.0])),
                geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
            });
            let (sql, _) = built(&compiled, &speed_and_address());
            assert!(
                !sql.contains(r#""dataset_id" IN"#) && !sql.contains(r#""dataset_id" IS NULL"#),
                "{selectors:?}: {sql}"
            );
            let count = built_count(&compiled, &speed_and_address());
            assert!(
                !count.contains(r#""dataset_id" IN"#) && !count.contains(r#""dataset_id" IS NULL"#),
                "{selectors:?}: {count}"
            );
        }
    }

    /// The Query Terms of an OR share one windowed existential, each arm
    /// pairing its own attribute key with its own test: an instance satisfying
    /// either term is what the disjunction asks for (clause 4.9).
    #[test]
    fn the_terms_of_an_or_share_one_windowed_existential() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">100|\"https://example.com/speed\"<10").unwrap());
        let (sql, _) = built(&compiled, &speed_and_address());
        pretty_assertions::assert_eq!(sql.matches(r#"AS "qh""#).count(), 1, "{sql}");
        assert!(
            sql.contains(r#"(("qh"."attribute_name_key" = $1 AND "qh"."value_number" > $2) OR ("qh"."attribute_name_key" = $3 AND "qh"."value_number" < $4))"#),
            "{sql}"
        );
        assert!(!sql.contains(") OR EXISTS(SELECT 1 FROM (SELECT"), "{sql}");
    }

    #[test]
    fn stage1_q_filter_on_an_unknown_attribute_is_unsatisfiable() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/never-written\">50").unwrap());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("FALSE"), "{sql}");
    }

    /// The attributes and sub-attributes the temporal `q` shapes below name.
    fn speed_address_and_their_sub_attributes() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("speed", 7), ("address", 8), ("accuracy", 9), ("method", 10), ("label", 11), ("isParkedIn", 12)].map(|(name, key)| {
                    (
                        format!("https://example.com/{name}").parse().expect("valid attribute name"),
                        AttributeNameKey::from(key),
                    )
                }),
            ),
            types: ResolvedNames::default(),
        }
    }

    /// Whether every `jsonb_build_object` of `sql` is built from operands and
    /// literals alone, never from a column of the history row a probe reads,
    /// which is what rebuilding that row as a document would take.
    fn builds_no_object_from_a_history_row(sql: &str) -> bool {
        sql.match_indices("jsonb_build_object(").all(|(start, call)| {
            let arguments = &sql[start + call.len()..];
            let end = arguments
                .char_indices()
                .scan(1_usize, |depth, (at, character)| {
                    *depth = if character == '(' {
                        *depth + 1
                    } else if character == ')' {
                        *depth - 1
                    } else {
                        *depth
                    };
                    Some((at, *depth))
                })
                .find_map(|(at, depth)| (depth == 0).then_some(at))
                .unwrap_or(arguments.len());
            !arguments[..end].contains(r#""qh"."#)
        })
    }

    /// Whether every jsonpath function of `sql` reads a stored `value_object`.
    fn reads_jsonpath_only_over_stored_values(sql: &str) -> bool {
        sql.split("jsonb_path_").skip(1).all(|call| {
            call.split_once('(')
                .and_then(|(_, arguments)| arguments.split_once(','))
                .is_some_and(|(target, _)| target.ends_with(r#"."value_object""#))
        })
    }

    /// Clause 4.9 reads a compound member as a `MemberExpression` of the target
    /// value, which a history row holds in `value_object` as current state
    /// does, so the member is read off the stored value.
    #[test]
    fn a_q_predicate_on_a_compound_value_reads_the_value_object() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/address\"[city]==\"Berlin\"").unwrap());
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(sql.contains(r#"jsonb_path_query("qh"."value_object""#), "{sql}");
        assert!(builds_no_object_from_a_history_row(&sql), "{sql}");
        assert!(!sql.contains("jsonb_strip_nulls"), "{sql}");
        assert!(reads_jsonpath_only_over_stored_values(&sql), "{sql}");
    }

    /// Clause 5.7.4.4 S2 checks every shape of Query Term against the stored
    /// in-window instances, which hold their values and their sub-attributes
    /// in columns and elements, so no shape rebuilds an instance as a document.
    #[test]
    fn no_temporal_statement_rebuilds_a_history_instance_document() {
        for q in [
            "\"https://example.com/speed\"==1",
            "\"https://example.com/speed\"!=1",
            "\"https://example.com/speed\">1",
            "\"https://example.com/speed\"==1..5",
            "\"https://example.com/speed\"==1,2",
            "\"https://example.com/speed\"~=\"^a\"",
            "\"https://example.com/speed\"!~=\"^a\"",
            "\"https://example.com/speed\"",
            "\"https://example.com/address\"[city]==\"Berlin\"",
            "\"https://example.com/address\"[city.name]==\"Bern\"",
            "\"https://example.com/address\"[en]==\"Main\"",
            "\"https://example.com/address\"[en]>\"M\"",
            "\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/observedAt\">\"2026-01-01T00:00:00Z\"",
            "\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/unitCode\"==\"KMH\"",
            "\"https://example.com/speed\".\"https://example.com/accuracy\">0.5",
            "\"https://example.com/speed\".\"https://example.com/accuracy\"",
            "\"https://example.com/speed\".\"https://example.com/accuracy\".\"https://example.com/method\"==\"gps\"",
            "\"https://example.com/speed\".\"https://example.com/accuracy\".\"https://uri.etsi.org/ngsi-ld/observedAt\">\"2026-01-01T00:00:00Z\"",
            "\"https://example.com/speed\".\"https://example.com/accuracy\"[c]==\"x\"",
            "\"https://example.com/speed\".\"https://example.com/label\"[en]==\"fast\"",
            "\"https://example.com/speed\".\"https://example.com/label\"~=\"^fa\"",
            "\"https://example.com/isParkedIn\"{\"https://example.com/address\"[city]}==\"Bern\"",
            "\"https://example.com/speed\">1|\"https://example.com/address\"[city]==\"Bern\"",
        ] {
            let mut compiled = minimal_compiled();
            compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
            compiled.entities.q_expr = Some(parser::parse(q).expect("parse q"));
            let (sql, _) = built(&compiled, &speed_address_and_their_sub_attributes());
            assert!(builds_no_object_from_a_history_row(&sql), "{q}: {sql}");
            assert!(!sql.contains("jsonb_strip_nulls") && !sql.contains("to_jsonb("), "{q}: {sql}");
            assert!(reads_jsonpath_only_over_stored_values(&sql), "{q}: {sql}");
        }
    }

    /// Clause 5.7.4.4 S3: the geospatial restriction is checked against the
    /// `GeoProperty` instances "within the interval defined by the temporal
    /// query", which are `attribute_history` rows -- not the single hoisted
    /// current-state `location` column, which holds one instant and answers a
    /// different question. The default `location` reaches its history rows
    /// through the stream just as any named `GeoProperty` does.
    #[test]
    fn a_default_location_geo_query_reads_the_windowed_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#") AS "ahg" JOIN "attribute_names" AS "ang""#), "{sql}");
        assert!(sql.contains(r#""ahg"."value_geometry""#), "{sql}");
        assert!(sql.contains(r#""re"."id" = "e"."id""#), "correlated to the temporal entity: {sql}");
        assert!(!sql.contains("entity_search"), "the geo restriction no longer reads current state: {sql}");
    }

    /// Clause 5.7.4.4 S3 restricts the geo instances to the temporal window, so
    /// the geo existential restates the window predicate on its own
    /// `attribute_history` axis rather than trusting the stream match alone.
    #[test]
    fn a_temporal_geo_query_restricts_the_geo_instances_to_the_window() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2020-07-01T12:05:00Z"));
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("2000").expect("positive distance")),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([-8.503, 41.202])),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#""ahg"."observed_at""#), "the window is applied to the geo instances: {sql}");
        assert!(sql.contains("ST_DWithin"), "near renders a distance relation on the geo instances: {sql}");
    }

    #[test]
    fn no_scope_q_unchanged() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(!sql.contains("scope_history"), "No scope_history reference without scopeQ: {sql}");
    }

    /// Clause 5.7.4.4 S3 applies to a named `GeoProperty` exactly as to the
    /// default `location`: its instances are the `attribute_history` rows of the
    /// stream carrying that name, restricted to the window, so a named property
    /// reaches the same history rows the default one does rather than the
    /// current-state projection.
    #[test]
    fn a_temporal_geo_query_on_a_named_geoproperty_reads_the_windowed_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/customGeo".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#") AS "ahg""#), "{sql}");
        assert!(
            sql.contains(r#"JOIN "attribute_names" AS "ang""#),
            "the target GeoProperty is named through the dictionary: {sql}"
        );
        assert!(sql.contains(r#""ahg"."value_geometry""#), "{sql}");
        assert!(
            !sql.contains("current_attributes"),
            "a temporal geo query reads history, not current state: {sql}"
        );
    }

    /// The IRI is reached by dictionary key, so an apostrophe in one never
    /// enters the statement text.
    #[test]
    fn a_temporal_geoproperty_iri_never_reaches_the_statement_text() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/o'brien".parse().expect("valid attribute name"),
        });
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([(
                "https://example.com/o'brien".parse().expect("valid attribute name"),
                AttributeNameKey::from(9_i32),
            )]),
            types: ResolvedNames::default(),
        };
        let (sql, _) = built(&compiled, &names);
        assert!(!sql.contains("brien"), "{sql}");
        // Every literal contributes a balanced pair of quotes, so the total is
        // even unless a literal was terminated early.
        assert_eq!(sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {sql}");
    }

    /// Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    /// the JSON-LD array returned shall be ordered according to the member
    /// names specified", and Table 6.18.3.2-1 makes the collation the
    /// comparison the payload "shall be ordered according to". The identifier
    /// column is `COLLATE "C"`, so the requested collation has to be written
    /// into the key or the ordering is byte-wise whatever was asked for.
    #[test]
    fn a_requested_ordering_collates_the_identifier_key() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering {
            direction: SortDirection::Ascending,
            collation: Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "de-u-co-phonebk" ASC NULLS LAST"#), "{sql}");
    }

    /// Clause 4.23.1 makes ICU "root" (`und-x-icu`) the default collation
    /// order, so an ordering that named none still names one rather than
    /// falling back to the column's byte-wise `COLLATE "C"`.
    #[test]
    fn an_ordering_without_a_collation_compares_under_the_icu_root_order() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Ascending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "und-x-icu" ASC NULLS LAST"#), "{sql}");
    }

    /// Table 6.18.3.2-1 gives `desc` as one of the two sorting styles.
    #[test]
    fn a_descending_ordering_reverses_the_identifier_key() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Descending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "und-x-icu" DESC NULLS LAST"#), "{sql}");
    }

    /// The clause 5.5.9 tiebreaker survives an explicit ordering, so a page
    /// boundary never falls inside a group the requested key left tied.
    #[test]
    fn the_stable_identifier_tiebreaker_follows_a_requested_ordering() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Descending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        let collated = sql.find("COLLATE").expect("the requested key is written");
        let tiebreaker = sql.rfind(r#""e"."id" ASC"#).expect("the stable key is written");
        assert!(collated < tiebreaker, "{sql}");
    }

    /// Table 6.18.3.2-1 makes `collation` "part of Entity ordering", so a query
    /// that ordered nothing keeps the store's own stable order and pays for no
    /// collation at all.
    #[test]
    fn a_query_that_requested_no_ordering_keeps_the_stable_identifier_order() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(!sql.contains("COLLATE"), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" ASC"#), "{sql}");
    }

    /// The count answers "how many", which no ordering changes, so clause 5.5.9
    /// gets its total without a sort the planner would have to perform.
    #[test]
    fn the_count_statement_carries_no_ordering() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Ascending).build());
        let sql = built_count(&compiled, &QueryNames::default());
        let outer = sql.rsplit(r#") AS "e""#).next().expect("the statement past its entity source");
        assert!(!outer.contains("ORDER BY"), "{sql}");
    }

    fn boundary(id: &str) -> Identifier {
        id.parse().expect("valid URI")
    }

    /// A cursor page of 20 starting at `cursor`.
    fn cursor_page(cursor: PageCursor) -> PageWindow {
        PageWindow {
            limit: 20,
            start: PageStart::Cursor(Some(cursor)),
        }
    }

    /// Clause 5.5.9.1: a forward cursor pages the element dimension past the
    /// boundary id over the default order -- `WHERE id > $x ORDER BY id ASC` and
    /// no `OFFSET`.
    #[test]
    fn stage1_keyset_after_pages_forward_ascending_without_offset() {
        let compiled = minimal_compiled();
        let (sql, _) = built_windowed(&compiled, &PageWindow::after(20, boundary("urn:ngsi-ld:Vehicle:001")), &QueryNames::default());
        assert!(sql.contains(r#""e"."id" > $"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" ASC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "a keyset page never skips: {sql}");
    }

    /// Clause 5.5.9.1: a backward cursor pages before the boundary over the
    /// reversed order -- `WHERE id < $x ORDER BY id DESC` -- which the store
    /// reverses to ascending after fetching.
    #[test]
    fn stage1_keyset_before_pages_backward_descending_without_offset() {
        let compiled = minimal_compiled();
        let (sql, _) = built_windowed(
            &compiled,
            &cursor_page(PageCursor::Before(boundary("urn:ngsi-ld:Vehicle:042"))),
            &QueryNames::default(),
        );
        assert!(sql.contains(r#""e"."id" < $"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" DESC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// Clause 5.5.9.1: the from-end cursor takes the final page -- `ORDER BY id
    /// DESC`, no boundary predicate -- which the store reverses to ascending.
    #[test]
    fn stage1_keyset_end_takes_the_final_page_descending() {
        let compiled = minimal_compiled();
        let (sql, _) = built_windowed(&compiled, &cursor_page(PageCursor::End), &QueryNames::default());
        assert!(sql.contains(r#"ORDER BY "e"."id" DESC"#), "{sql}");
        assert!(!sql.contains(r#""e"."id" >"#) && !sql.contains(r#""e"."id" <"#), "no boundary predicate: {sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// The count statement ignores the keyset, so a counted cursor page still
    /// reports the whole collection's total rather than the remaining tail.
    #[test]
    fn a_keyset_count_still_counts_the_whole_collection() {
        let compiled = minimal_compiled();
        let sql = built_windowed_count(&compiled, &PageWindow::after(20, boundary("urn:ngsi-ld:Vehicle:001")), &QueryNames::default());
        assert!(!sql.contains(r#""e"."id" >"#), "the count is not narrowed by the cursor: {sql}");
    }

    /// Clause 5.7.4: on the live branch an entity is selected by its queued
    /// entries as by its recorded rows, so stage 1 reads both.
    #[test]
    fn a_live_stage_one_reads_the_pending_entries_beside_the_history() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        let history = sql.split(r#") AS "ah" JOIN "attribute_names""#).next().expect("the history source");
        assert!(history.contains(r#"UNION ALL (SELECT "pe"."entity_id" AS "entity_id""#), "{sql}");
        assert!(history.contains(r#"FROM "temporal_append_entries" AS "pe""#), "{sql}");
    }

    /// A Snapshot branch records its history in the write's own transaction
    /// and has no queue, so its stage 1 reads the recorded relations alone.
    #[test]
    fn a_snapshot_stage_one_reads_no_pending_entry() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let snapshot = Alias::new("snap_7").into_iden();
        let stage1 = build_stage1_entity_ids(
            &HistoryBranch::recorded_in(snapshot),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Emit,
            &QueryNames::default(),
        )
        .expect("valid temporal query");
        for sql in [
            stage1.select.to_string(PostgresQueryBuilder),
            stage1.count.expect("count requested").to_string(PostgresQueryBuilder),
        ] {
            assert!(!sql.contains("temporal_append"), "{sql}");
            assert!(sql.contains(r#"FROM "snap_7"."temporal_entities" AS "e""#), "{sql}");
        }
    }

    /// The page and the count of a stage 1 over `branch` for a window opening
    /// at `opening`, or never closing when `opening` is `None`.
    fn page_and_count(branch: &HistoryBranch, opening: Option<&str>) -> [String; 2] {
        let mut compiled = minimal_compiled();
        if let Some(opening) = opening {
            compiled.window.interval = TimeInterval::After(instant(opening));
        }
        let stage1 =
            build_stage1_entity_ids(branch, &compiled, &PageWindow::offset(20, 0), CountPolicy::Emit, &QueryNames::default()).expect("valid temporal query");
        [
            stage1.select.to_string(PostgresQueryBuilder),
            stage1.count.expect("count requested").to_string(PostgresQueryBuilder),
        ]
    }

    #[test]
    fn a_window_match_over_an_empty_queue_compares_the_evolution_beside_the_recorded_rows() {
        for branch in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(Alias::new("snap_7").into_iden())] {
            for sql in page_and_count(&branch, Some("2024-01-01T00:00:00Z")) {
                assert!(
                    sql.contains(r#") AS "ah" WHERE "ah"."observed_at" >= '2024-01-01 00:00:00.000000 +00:00' AND "ah"."entity_id" = "e"."id")"#),
                    "{sql}"
                );
                assert!(!sql.contains(r#""re"."id" = "e"."id""#), "{sql}");
            }
        }
    }

    #[test]
    fn a_stream_match_over_an_empty_queue_compares_the_evolution_beside_the_recorded_streams() {
        for sql in page_and_count(&HistoryBranch::live_recorded(), None) {
            assert!(sql.contains(r#") AS "s" WHERE TRUE AND "s"."entity_id" = "e"."id")"#), "{sql}");
            assert!(!sql.contains(r#""re"."id" = "e"."id""#), "{sql}");
        }
    }

    /// PostgreSQL pushes a restriction into the branches of a union, never a
    /// join clause, so a source reading the queue names the Evolution in both.
    #[test]
    fn a_window_match_reading_the_queue_takes_the_evolution_inside_both_branches() {
        for sql in page_and_count(&HistoryBranch::live(), Some("2024-01-01T00:00:00Z")) {
            assert!(sql.contains(r#""re"."id" = "e"."id""#), "{sql}");
            assert!(sql.contains(r#""pe"."entity_id" = "e"."id""#), "{sql}");
            assert!(!sql.contains(r#""ah"."entity_id" = "e"."id""#), "{sql}");
        }
    }

    /// Clause 6.3.13: the count is the number of entities the page is taken
    /// from, so both read the same source.
    #[test]
    fn the_count_reads_the_same_source_as_the_page() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let (page, _) = built(&compiled, &QueryNames::default());
        let count = built_count(&compiled, &QueryNames::default());
        let source = |sql: &str| {
            let from = sql.find(" FROM ").expect("a FROM clause");
            let entities = sql.rfind(r#") AS "e""#).expect("the entity source");
            let tail = [" ORDER BY ", " LIMIT "]
                .iter()
                .filter_map(|clause| sql[entities..].find(clause).map(|at| entities + at))
                .min()
                .unwrap_or(sql.len());
            sql[from..tail].to_owned()
        };
        pretty_assertions::assert_eq!(source(&page), source(&count));
    }
}
