//! Entity access-path plans: each query shape reaches its intended index.
//!
//! Each representative query shape is planned against a seeded, analyzed corpus
//! and must reach its intended index, every typed-value index must exclude the
//! rows whose value column is null, and no index covers the version token
//! every write sets.

use crate::seeded_corpus::{PlannerPaths, SEED_PREFIX, SEEDED_ENTITIES, explain, explain_rolled_back, owner_object, seed, seed_essays};
use andromeda_postgresql_schema::current::{CurrentAttributes, Entities, EntitySearch, EntityTypeMembers};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    catalog_function::expression_definition,
    clock::now,
    jsonb::{jsonb_build_array, jsonb_build_object},
    literal::{int_literal, true_literal},
    ltree::ltree_matches,
    operator::{array_contains, array_contains_key, regex_match},
    pg_array::{any, array_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    postgis::{bounding_boxes_overlap, st_dwithin, st_intersects, st_make_envelope, st_point_wgs84},
    statement::Statement,
    system_catalog::PgClass,
};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use chrono::Utc;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, Iden, JoinType, Order, Query, SelectStatement, UnionType};
use sqlx::PgPool;

/// Alias of the sort key a page is cut on.
const SORT_KEY: &str = "sort_0";

/// Entity keys a Query Entities page holds.
const PAGE_SIZE: u64 = 100;

/// One planned query shape: what it selects, the statement that selects it, and
/// the index the schema exists to give it.
struct PlannedShape {
    /// Name reported when the plan misses its index.
    shape: &'static str,
    /// Statement handed to `EXPLAIN`.
    statement: SelectStatement,
    /// Index the plan must use.
    index: &'static str,
}

/// Column `name` of the relation aliased `relation`.
fn column(relation: &'static str, name: impl Iden + 'static) -> Expr {
    Expr::col((Alias::new(relation), name))
}

/// The WGS 84 point every geo shape probes with.
fn probe_point() -> Expr {
    st_point_wgs84(Expr::val(13.005), Expr::val(52.005))
}

/// Whether the row aliased `relation` has not expired: no expiry, or one in the
/// future.
fn unexpired(relation: &'static str, expiry: impl Iden + Copy + 'static) -> Expr {
    column(relation, expiry).is_null().or(column(relation, expiry).gt(now()))
}

/// The identity row `e` joined to its search row `s`.
fn entities_with_search() -> SelectStatement {
    Query::select()
        .from_as(Entities::Table, Alias::new("e"))
        .join_as(
            JoinType::InnerJoin,
            EntitySearch::Table,
            Alias::new("s"),
            column("s", EntitySearch::EntityKey).equals((Alias::new("e"), Entities::EntityKey)),
        )
        .to_owned()
}

/// An `EXISTS` probe over the attribute rows `c` of the entity `e`, narrowed by
/// `condition`.
fn attribute_probe(condition: Expr) -> Expr {
    Expr::exists(
        Query::select()
            .expr(int_literal(1))
            .from_as(CurrentAttributes::Table, Alias::new("c"))
            .and_where(column("c", CurrentAttributes::EntityKey).equals((Alias::new("e"), Entities::EntityKey)))
            .and_where(condition)
            .to_owned(),
    )
}

/// The shapes that read the identity row `e` or its search row `s`.
fn entity_table_shapes(prefix: &str, alarm: i32) -> Vec<PlannedShape> {
    vec![
        PlannedShape {
            shape: "anchored id pattern",
            statement: Query::select()
                .expr(column("e", Entities::Id))
                .from_as(Entities::Table, Alias::new("e"))
                .and_where(column("e", Entities::Id).like(format!("{prefix}:seed:0001%")))
                .to_owned(),
            index: "uq_entities_id",
        },
        PlannedShape {
            shape: "unanchored id pattern",
            statement: Query::select()
                .expr(column("e", Entities::Id))
                .from_as(Entities::Table, Alias::new("e"))
                .and_where(regex_match(column("e", Entities::Id), Expr::val(format!("{prefix}:seed:01234"))))
                .to_owned(),
            index: "idx_entities_id_trgm",
        },
        PlannedShape {
            shape: "scope path",
            statement: Query::select()
                .expr(column("s", EntitySearch::EntityKey))
                .from_as(EntitySearch::Table, Alias::new("s"))
                .and_where(ltree_matches(column("s", EntitySearch::ScopePaths), Expr::val("seed.5")))
                .to_owned(),
            index: "idx_entity_search_scope_paths",
        },
        PlannedShape {
            shape: "default-location geo",
            statement: Query::select()
                .expr(column("s", EntitySearch::EntityKey))
                .from_as(EntitySearch::Table, Alias::new("s"))
                .and_where(st_intersects(column("s", EntitySearch::Location), probe_point()))
                .to_owned(),
            index: "idx_entity_search_location",
        },
        PlannedShape {
            shape: "rare attrs term",
            statement: Query::select()
                .expr(column("s", EntitySearch::EntityKey))
                .from_as(EntitySearch::Table, Alias::new("s"))
                .and_where(array_contains_key(column("s", EntitySearch::AttributeNameKeys), alarm))
                .to_owned(),
            index: "idx_entity_search_attribute_name_keys",
        },
        // The page shape the entity list issues: ordered by identifier, limited,
        // heads read by key. A key the planner could not estimate would make
        // it walk the identifier index and filter every row instead of probing
        // the GIN; the inlined key is what keeps the probe.
        PlannedShape {
            shape: "rare attrs page",
            statement: head_page(
                entities_with_search()
                    .and_where(array_contains_key(column("s", EntitySearch::AttributeNameKeys), alarm))
                    .to_owned(),
                0,
            ),
            index: "idx_entity_search_attribute_name_keys",
        },
        PlannedShape {
            shape: "attribute-expiry sweep",
            statement: Query::select()
                .columns([Entities::EntityKey, Entities::Id, Entities::ModifiedAt].map(|name| (Alias::new("e"), name)))
                .from_as(Entities::Table, Alias::new("e"))
                .and_where(column("e", Entities::MinAttributeExpiresAt).is_not_null())
                .and_where(column("e", Entities::MinAttributeExpiresAt).lte(Utc::now()))
                .order_by((Alias::new("e"), Entities::MinAttributeExpiresAt), Order::Asc)
                .order_by((Alias::new("e"), Entities::Id), Order::Asc)
                .limit(PAGE_SIZE)
                .to_owned(),
            index: "idx_entities_min_attribute_expires_at",
        },
    ]
}

/// The shapes that read the attribute table `c`, alone or through an `EXISTS`
/// probe from the identity row.
fn attribute_table_shapes(prefix: &str, reading: i32, entrance: i32, city: i32) -> Vec<PlannedShape> {
    vec![
        PlannedShape {
            shape: "arbitrary GeoProperty geo",
            statement: Query::select()
                .expr(column("c", CurrentAttributes::EntityKey))
                .from_as(CurrentAttributes::Table, Alias::new("c"))
                .and_where(column("c", CurrentAttributes::AttributeNameKey).eq(entrance))
                .and_where(st_intersects(column("c", CurrentAttributes::ValueGeometry), probe_point()))
                .to_owned(),
            index: "idx_ca_value_geometry",
        },
        PlannedShape {
            shape: "attribute existence",
            statement: Query::select()
                .expr(column("e", Entities::Id))
                .from_as(Entities::Table, Alias::new("e"))
                .and_where(column("e", Entities::Id).like(format!("{prefix}:seed:00001%")))
                .and_where(attribute_probe(column("c", CurrentAttributes::AttributeNameKey).eq(reading)))
                .to_owned(),
            index: "uq_current_attributes_grain",
        },
        PlannedShape {
            shape: "numeric q",
            statement: Query::select()
                .expr(column("e", Entities::Id))
                .from_as(Entities::Table, Alias::new("e"))
                .and_where(attribute_probe(
                    column("c", CurrentAttributes::AttributeNameKey)
                        .eq(reading)
                        .and(column("c", CurrentAttributes::ValueNumber).eq(cast(Expr::val(42), PostgresType::Numeric))),
                ))
                .to_owned(),
            index: "idx_ca_value_number",
        },
        PlannedShape {
            shape: "unanchored string pattern q",
            statement: Query::select()
                .expr(column("c", CurrentAttributes::EntityKey))
                .from_as(CurrentAttributes::Table, Alias::new("c"))
                .and_where(column("c", CurrentAttributes::AttributeNameKey).eq(city))
                .and_where(regex_match(column("c", CurrentAttributes::ValueString), Expr::val("ty-12")))
                .to_owned(),
            index: "idx_ca_value_string_trgm",
        },
    ]
}

#[tokio::test]
async fn entity_access_paths_reach_their_intended_index_for_every_query_shape() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let mut shapes = entity_table_shapes(corpus.prefix(), corpus.alarm());
    shapes.extend(attribute_table_shapes(corpus.prefix(), corpus.reading(), corpus.entrance(), corpus.city()));

    for shape in shapes {
        let plan = explain(pool, shape.statement).await;
        assert!(plan.contains(shape.index), "the {} shape must use {}: {plan}", shape.shape, shape.index);
    }

    scope.finish().await.expect("cleanup failed");
}

/// Count of the seeded Entities satisfying `filter` over the identity row `e`
/// and its search row `s`.
fn seeded_count(prefix: &str, filter: Expr) -> SelectStatement {
    entities_with_search()
        .expr(count_all())
        .and_where(filter)
        .and_where(column("e", Entities::Id).like(format!("{prefix}:seed:%")))
        .to_owned()
}

/// Run a count statement.
async fn count(pool: &PgPool, statement: SelectStatement) -> i64 {
    statement.to_query_scalar::<i64>().fetch_one(pool).await.expect("the count must be readable")
}

#[tokio::test]
async fn entity_access_path_for_a_type_filter_uses_the_types_gin_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let type_keys = || column("s", EntitySearch::EntityTypeNameKeys);

    // The type filter is the one shape whose compiled form decides whether
    // `idx_entity_search_entity_type_name_keys` has a consumer at all: GIN `array_ops` serves
    // `&&`, `@>` and `<@`, and does not serve `= ANY(entity_type_name_keys)`. This is the
    // containment shape the type-selection rendering of clause 4.17 compiles
    // to, with the resolved dictionary key inlined as an array literal so the
    // planner estimates it from the column's element statistics.
    let compiled = Query::select()
        .expr(column("s", EntitySearch::EntityKey))
        .from_as(EntitySearch::Table, Alias::new("s"))
        .and_where(array_contains_key(type_keys(), corpus.rare_type()))
        .to_owned();
    let plan = explain(pool, compiled).await;
    assert!(
        plan.contains("idx_entity_search_entity_type_name_keys"),
        "a type filter must reach the entity_type_name_keys GIN index: {plan}"
    );

    // The same predicate must select the same Entities however it is planned,
    // so the residual oracle counts with the equality spelling the index cannot
    // answer and the two must agree.
    let selective = count(pool, seeded_count(corpus.prefix(), Expr::val(corpus.rare_type()).eq(any(type_keys())))).await;
    let contained = count(
        pool,
        seeded_count(
            corpus.prefix(),
            array_contains(type_keys(), Expr::val(corpus.rare_type()), PostgresType::Integer),
        ),
    )
    .await;
    assert_eq!(selective, contained, "containment and equality must select the same Entities");
    let common = count(
        pool,
        seeded_count(
            corpus.prefix(),
            array_contains(type_keys(), Expr::val(corpus.common_type()), PostgresType::Integer),
        ),
    )
    .await;
    assert_eq!(selective + common, SEEDED_ENTITIES);

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 4.17 and 5.5.9: a page of one type walks that type's membership key
/// range in identifier order, so its cost follows the rows it skips and keeps
/// and never the Entities of other types whose identifiers sort ahead of it,
/// which a walk of `uq_entities_id` filtered by type would read. The
/// memberships select exactly the Entities the search row's types do.
#[tokio::test]
async fn a_type_page_walks_the_membership_key_in_identifier_order() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

    for type_key in [corpus.rare_type(), corpus.common_type()] {
        let members_of_type = Query::select()
            .columns([EntityTypeMembers::EntityKey, EntityTypeMembers::Id])
            .from(EntityTypeMembers::Table)
            .and_where(Expr::col(EntityTypeMembers::EntityTypeNameKey).eq(type_key))
            .to_owned();
        let walk = Query::select().from_subquery(members_of_type, Alias::new("e")).to_owned();
        let plan = explain(pool, head_page(walk, 10)).await;
        assert!(plan.contains("pk_entity_type_members"), "the page walks the membership key: {plan}");
        assert!(!plan.contains("uq_entities_id"), "the page reads no other type's identifiers: {plan}");
        assert!(!plan.contains("Sort "), "the walk yields identifier order, so nothing is sorted: {plan}");

        let members = count(
            pool,
            Query::select()
                .expr(count_all())
                .from(EntityTypeMembers::Table)
                .and_where(Expr::col(EntityTypeMembers::EntityTypeNameKey).eq(type_key))
                .to_owned(),
        )
        .await;
        let searched = count(
            pool,
            Query::select()
                .expr(count_all())
                .from(EntitySearch::Table)
                .and_where(Expr::col(EntitySearch::EntityTypeNameKeys).array_contains(array_literal([cast(Expr::val(type_key), PostgresType::Integer)])))
                .to_owned(),
        )
        .await;
        assert_eq!(members, searched, "the memberships and the search rows select the same Entities");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The instance rows of the attribute `instances` that carry a `datasetId` and
/// a geometry, keyed by entity.
fn geometry_instances(instances: i32) -> SelectStatement {
    Query::select()
        .expr(column("c", CurrentAttributes::EntityKey))
        .from_as(CurrentAttributes::Table, Alias::new("c"))
        .and_where(column("c", CurrentAttributes::AttributeNameKey).eq(instances))
        .and_where(column("c", CurrentAttributes::DatasetId).is_not_null())
        .and_where(column("c", CurrentAttributes::ValueGeometry).is_not_null())
        .to_owned()
}

/// The entities whose hoisted default location is selected by `condition`.
fn hoisted_locations(condition: Expr) -> SelectStatement {
    Query::select()
        .expr(column("geo_default", EntitySearch::EntityKey))
        .from_as(EntitySearch::Table, Alias::new("geo_default"))
        .and_where(condition)
        .to_owned()
}

/// Every entity of `arms` as a semi-join over the identity row joined to its
/// search row.
fn semi_joined(arms: SelectStatement) -> SelectStatement {
    entities_with_search()
        .expr(column("e", Entities::Id))
        .and_where(column("e", Entities::EntityKey).in_subquery(arms))
        .to_owned()
}

/// Clause 5.7.2.4 lets any `location` instance match, so a `location`
/// geo-query is a semi-join over two arms: the hoisted default instance and
/// every instance row carrying a `datasetId`. Each arm must reach its own `GiST`
/// index, or the common single-instance query would lose the hoisted path. The
/// seeded `entrance` rows stand in
/// for the `location` instance rows: the arm reads them by the same key and
/// columns.
#[tokio::test]
async fn every_arm_of_a_location_geo_query_reaches_its_gist_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let instances = corpus.entrance();

    let mut hoisted_within = hoisted_locations(
        unexpired("geo_default", EntitySearch::LocationExpiresAt).and(st_intersects(column("geo_default", EntitySearch::Location), probe_point())),
    );
    let within = semi_joined(
        hoisted_within
            .union(
                UnionType::All,
                geometry_instances(instances)
                    .and_where(unexpired("c", CurrentAttributes::ExpiresAt))
                    .and_where(st_intersects(column("c", CurrentAttributes::ValueGeometry), probe_point()))
                    .to_owned(),
            )
            .to_owned(),
    );
    let plan = explain(pool, within).await;
    assert!(plan.contains("idx_entity_search_location"), "the hoisted arm must use its GiST index: {plan}");
    assert!(plan.contains("idx_ca_value_geometry"), "the instance arm must use its GiST index: {plan}");

    // `near` reads the hoisted `geography` twin, and prefilters the cast
    // instance geometry with a box the geometry GiST answers.
    let mut hoisted_near = hoisted_locations(st_dwithin(
        column("geo_default", EntitySearch::LocationGeography),
        cast(probe_point(), PostgresType::Geography),
        50.0,
    ));
    let near = semi_joined(
        hoisted_near
            .union(
                UnionType::All,
                geometry_instances(instances)
                    .and_where(bounding_boxes_overlap(
                        column("c", CurrentAttributes::ValueGeometry),
                        st_make_envelope([13.004, 52.004], [13.006, 52.006]),
                    ))
                    .and_where(st_dwithin(
                        cast(column("c", CurrentAttributes::ValueGeometry), PostgresType::Geography),
                        cast(probe_point(), PostgresType::Geography),
                        50.0,
                    ))
                    .to_owned(),
            )
            .to_owned(),
    );
    let plan = explain(pool, near).await;
    assert!(
        plan.contains("idx_entity_search_location_geography"),
        "the hoisted arm must use its geography GiST index: {plan}"
    );
    assert!(
        plan.contains("idx_ca_value_geometry"),
        "the prefiltered instance arm must use its GiST index: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The value test of a compiled clause 4.9 string equality: every column the
/// target value can occupy, the structured column by containment.
fn string_equality(operand: &str) -> Expr {
    let text = || cast(Expr::val(operand), PostgresType::Text);
    let structured = column("c", CurrentAttributes::ValueObject);
    column("c", CurrentAttributes::ValueString)
        .eq(operand)
        .or(column("c", CurrentAttributes::ValueUri).eq(operand))
        .or(structured.array_contains(jsonb_build_array(text())))
        .or(column("c", CurrentAttributes::ValueObject).array_contains(jsonb_build_array(jsonb_build_object([Expr::val("@value"), text()]))))
}

/// The head statement a Query Entities page issues over `source` (its `FROM`
/// and `WHERE`, the identity row aliased `e`): the page of 100 entity keys is
/// cut in identifier order past `offset` rows, and each kept row's head is
/// looked up by key outside the window; its attribute rows are read by key
/// afterwards.
fn head_page(mut source: SelectStatement, offset: u64) -> SelectStatement {
    let page = source
        .expr(column("e", Entities::EntityKey))
        .expr_as(column("e", Entities::Id), Alias::new(SORT_KEY))
        .order_by(Alias::new(SORT_KEY), Order::Asc)
        .limit(PAGE_SIZE)
        .offset(offset)
        .to_owned();
    let lookup = Query::select()
        .columns(
            [
                Entities::EntityKey,
                Entities::Id,
                Entities::CreatedAt,
                Entities::ModifiedAt,
                Entities::ExpiresAt,
            ]
            .map(|name| (Alias::new("pe"), name)),
        )
        .from_as(Entities::Table, Alias::new("pe"))
        .and_where(column("pe", Entities::EntityKey).equals((Alias::new("page"), Entities::EntityKey)))
        .limit(1)
        .to_owned();
    Query::select()
        .columns(
            [
                Entities::EntityKey,
                Entities::Id,
                Entities::CreatedAt,
                Entities::ModifiedAt,
                Entities::ExpiresAt,
            ]
            .map(|name| (Alias::new("stored"), name)),
        )
        .from_subquery(page, Alias::new("page"))
        .join_lateral(JoinType::InnerJoin, lookup, Alias::new("stored"), true_literal())
        .order_by((Alias::new("page"), Alias::new(SORT_KEY)), Order::Asc)
        .to_owned()
}

/// The page shape a Query Entities `q` issues: ordered by identifier, limited,
/// with the probe the compiler emits.
fn q_page(probe: Expr) -> SelectStatement {
    head_page(
        entities_with_search()
            .and_where(attribute_probe(unexpired("c", CurrentAttributes::ExpiresAt).and(probe)))
            .to_owned(),
        0,
    )
}

/// A page past an `OFFSET` reads the heads of the rows it keeps and no others:
/// the head lookup runs outside the window, once per kept row,
/// rather than once for every row the offset walks past. Whether the outer
/// statement re-sorts the page or the planner carries its order through, the
/// rows come back in the order the page was cut in (clause 4.23.1).
#[tokio::test]
async fn an_offset_page_reads_the_heads_of_its_kept_rows_only() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed(pool, SEED_PREFIX).await;

    let all_entities = || Query::select().from_as(Entities::Table, Alias::new("e")).to_owned();
    let plan = explain(pool, head_page(all_entities(), 5_000)).await;
    let lookup = plan
        .lines()
        .find(|line| line.contains("pk_entities"))
        .unwrap_or_else(|| panic!("the head is looked up by key: {plan}"));
    assert!(lookup.contains("loops=100)"), "one lookup per kept row, not per skipped row: {plan}");

    let window = all_entities()
        .expr(column("e", Entities::Id))
        .expr_as(column("e", Entities::Id), Alias::new(SORT_KEY))
        .order_by(Alias::new(SORT_KEY), Order::Asc)
        .limit(PAGE_SIZE)
        .offset(5_000)
        .to_owned();
    let ids: Vec<String> = Query::select()
        .column((Alias::new("page"), Entities::Id))
        .from_subquery(window, Alias::new("page"))
        .order_by((Alias::new("page"), Alias::new(SORT_KEY)), Order::Asc)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the page must be readable");
    let mut sorted = ids.clone(); // clone: compared against the order the statement returned
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(ids.len(), 100);

    scope.finish().await.expect("cleanup failed");
}

/// A clause 4.9 equality tests every column its target value can occupy, the
/// structured one by containment, so the probe is driven from the value indexes
/// only when every arm has one: a `BitmapOr` over the typed B-trees and the
/// `value_object` GIN. A disjunction of Query Terms shares that one probe, each
/// arm keeping its own attribute key, so it is driven the same way instead of
/// filtering every Entity in identifier order.
#[tokio::test]
async fn a_q_equality_and_a_q_disjunction_are_driven_from_every_value_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let owner = corpus.owner();

    let first = owner_object(7);
    let second = owner_object(11);
    let owned_by = |object: &str| column("c", CurrentAttributes::AttributeNameKey).eq(owner).and(string_equality(object));
    let shapes = [("equality", owned_by(&first)), ("disjunction", owned_by(&first).or(owned_by(&second)))];
    for (shape, probe) in shapes {
        let plan = explain(pool, q_page(probe)).await;
        for fragment in ["BitmapOr", "idx_ca_value_uri", "idx_ca_value_string ", "idx_ca_value_object"] {
            assert!(plan.contains(fragment), "the {shape} probe must use {fragment}: {plan}");
        }
        assert!(!plan.contains("uq_entities_id"), "the {shape} probe must not walk every Entity: {plan}");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_value_indexes_exclude_rows_whose_value_column_is_null() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    for (index, column) in [
        ("idx_ca_value_number", "value_number"),
        ("idx_ca_value_string", "value_string"),
        ("idx_ca_value_string_trgm", "value_string"),
        ("idx_ca_value_boolean", "value_boolean"),
        ("idx_ca_value_date_time", "value_date_time"),
        ("idx_ca_value_uri", "value_uri"),
        ("idx_ca_value_long_text", "value_long_text"),
        ("idx_ca_value_long_text_trgm", "value_long_text"),
        ("idx_ca_value_long_uri", "value_long_uri"),
        ("idx_ca_value_object", "value_object"),
        ("idx_ca_value_geometry", "value_geometry"),
        ("idx_ca_entity_geometry", "value_geometry"),
    ] {
        let predicate = index_predicate(pool, index).await;
        assert_eq!(predicate, Some(format!("({column} IS NOT NULL)")), "{index} must be partial on {column}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The attribute rows of `attribute` whose string or overflow-string column
/// satisfies `condition`.
fn string_columns_of(attribute: i32, condition: Expr) -> SelectStatement {
    Query::select()
        .expr(column("c", CurrentAttributes::EntityKey))
        .from_as(CurrentAttributes::Table, Alias::new("c"))
        .and_where(column("c", CurrentAttributes::AttributeNameKey).eq(attribute))
        .and_where(condition)
        .to_owned()
}

/// A String past the 2048-octet bound is held in `value_long_text`, which no
/// B-tree can key, so the overflow column has its own pair of indexes: the
/// attribute-aware trigram GIN serves the clause 4.9 patterns, and the partial
/// B-tree on the attribute alone serves an ordering, whose value is then
/// filtered on the heap. The compiled pattern tests both string columns as one
/// disjunction, and each arm must reach its own trigram index.
///
/// The lone pattern names no attribute on purpose. The partial B-tree's
/// predicate and an attribute equality select the same rows, but the planner
/// multiplies their selectivities as if they were independent, so an
/// attribute-qualified pattern over one column underestimates the B-tree path
/// and filters every overflow row of the attribute on the heap instead. The
/// attribute-qualified disjunction, which is the shape the compiler emits,
/// reaches both trigram indexes at this population all the same.
#[tokio::test]
async fn long_string_access_paths_use_the_overflow_indexes() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    let essays = seed_essays(pool, &corpus).await;
    let attribute = essays.attribute();
    let probe = essays.probe();

    let pattern = explain(
        pool,
        Query::select()
            .expr(column("c", CurrentAttributes::EntityKey))
            .from_as(CurrentAttributes::Table, Alias::new("c"))
            .and_where(regex_match(column("c", CurrentAttributes::ValueLongText), Expr::val(probe)))
            .to_owned(),
    )
    .await;
    assert!(
        pattern.contains("idx_ca_value_long_text_trgm"),
        "an unanchored pattern over long strings must use the overflow trigram index: {pattern}"
    );

    let disjunction = explain(
        pool,
        string_columns_of(
            attribute,
            regex_match(column("c", CurrentAttributes::ValueString), Expr::val(probe))
                .or(regex_match(column("c", CurrentAttributes::ValueLongText), Expr::val(probe))),
        ),
    )
    .await;
    assert!(
        disjunction.contains("BitmapOr") && disjunction.contains("idx_ca_value_string_trgm") && disjunction.contains("idx_ca_value_long_text_trgm"),
        "the compiled pattern must OR one bitmap per string column's trigram index: {disjunction}"
    );

    // The trailing space is the gap before the cost estimate, so the trigram
    // index, whose name extends this one, cannot satisfy the assertion.
    let ordering = explain_rolled_back(
        pool,
        PlannerPaths::WithoutSequentialScans,
        string_columns_of(
            attribute,
            column("c", CurrentAttributes::ValueString)
                .gt("a")
                .or(column("c", CurrentAttributes::ValueLongText).gt("a")),
        ),
    )
    .await;
    assert!(
        ordering.contains("idx_ca_value_long_text "),
        "an ordering over long strings must be driven from the overflow B-tree: {ordering}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The `attrs` axis is answered by the attribute keys the write path records
/// on the search row, so a term selects exactly the entities whose rows carry
/// the member: every seeded entity for the common attribute, one in a
/// thousand for the rare one.
#[tokio::test]
async fn attrs_terms_select_by_the_recorded_attribute_keys() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

    let carrying = |attribute: i32| {
        seeded_count(
            corpus.prefix(),
            array_contains(column("s", EntitySearch::AttributeNameKeys), Expr::val(attribute), PostgresType::Integer),
        )
    };
    assert_eq!(count(pool, carrying(corpus.city())).await, SEEDED_ENTITIES);
    assert_eq!(count(pool, carrying(corpus.alarm())).await, SEEDED_ENTITIES / 1000);

    scope.finish().await.expect("cleanup failed");
}

/// Every write sets `entities.modified_at`, so no index of the identity or
/// search row may cover it: the update stays heap-only and a value-only write
/// can never touch a search index.
#[tokio::test]
async fn no_index_covers_the_version_token() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    let indexed_definitions: Vec<String> = Query::select()
        .column(Alias::new("indexdef"))
        .from(Alias::new("pg_indexes"))
        .and_where(Expr::col(Alias::new("schemaname")).eq("public"))
        .and_where(Expr::col(Alias::new("tablename")).is_in(["entities", "entity_search"]))
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the index catalogue must be readable");
    for definition in &indexed_definitions {
        assert!(!definition.contains("modified_at"), "{definition}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Read the partial-index predicate of one index by name.
async fn index_predicate(pool: &PgPool, index: &str) -> Option<String> {
    Query::select()
        .expr(expression_definition(
            Expr::col((Alias::new("i"), Alias::new("indpred"))),
            Expr::col((Alias::new("i"), Alias::new("indrelid"))),
        ))
        .from_as(Alias::new("pg_index"), Alias::new("i"))
        .join_as(
            JoinType::InnerJoin,
            PgClass::Table,
            Alias::new("c"),
            Expr::col((Alias::new("c"), PgClass::Oid)).equals((Alias::new("i"), Alias::new("indexrelid"))),
        )
        .and_where(Expr::col((Alias::new("c"), PgClass::Relname)).eq(index))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the index must exist")
}
