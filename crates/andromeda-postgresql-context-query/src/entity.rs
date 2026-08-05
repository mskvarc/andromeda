//! SQL generation for entity queries using sea-query.

use crate::order_geography::geo_extract_geography;
use andromeda_geometry::ewkb::encode;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    error::QueryError,
    geo::query::GeoQuery,
    ordering::{
        collation::IcuCollation,
        geometry::OrderingGeometry,
        term::{CompiledOrderTerm, OrderDirection, OrderTarget},
    },
    pagination::CountPolicy,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::VALUE;
use andromeda_postgresql_query::{
    bind::sea_values_to_bind_values,
    built::BuiltQuery,
    error::RenderError,
    geo_predicate as geo_sql,
    geo_predicate::{GeometrySource, NullGeometryPolicy},
    jsonb::{escape_jsonpath, escape_jsonpath_key, jsonb_key_exists},
    operator::regex_match,
    q_predicate::{attribute_names::ResolvedAttributeNames, projection as q_filter, target::QFilterTarget},
    schema::Entities,
    scope_predicate::{ScopeTarget, condition as scope_condition},
    type_predicate::{TypesColumn, condition as type_condition},
};
use sea_query::{Alias, Asterisk, Condition, Expr as SeaExpr, Func, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

/// Builds a parameterized SQL query to retrieve entity payloads from the `entities` table.
///
/// Generates filter conditions from the compiled query (IDs, types, attrs, q expression,
/// geo filter, scope query), ordering, and pagination clauses.
pub fn build_query(query: &CompiledEntityQuery, attribute_names: &ResolvedAttributeNames) -> Result<BuiltQuery<'static>, RenderError> {
    let condition = build_entity_conditions(query, attribute_names)?;

    let mut select = Query::select();
    // clone: the fallback count statement filters on the same condition
    select.column(Entities::Payload).from(Entities::Table).cond_where(condition.clone());

    // When a total count is requested, piggyback it on every row as
    // `COUNT(*) OVER ()` so the common case runs in a single round-trip.
    // The count column is constant across the result set (PostgreSQL evaluates
    // the unbounded partition once), so the repository reads it from any row.
    // A zero-row result still needs the fallback `count_sql` because the
    // window function returns no rows when the filter matches nothing.
    if query.pagination.count == CountPolicy::Emit {
        select.expr_as(SeaExpr::cust("COUNT(*) OVER ()"), Alias::new(TOTAL_COUNT_COLUMN));
    }

    select.limit(query.pagination.limit as u64).offset(query.pagination.offset as u64);

    add_order_clause(
        &mut select,
        &query.ordering.terms,
        query.selection.geo.as_ref(),
        query.ordering.geometry.as_ref(),
    )?;

    let (sql, values) = select.build(PostgresQueryBuilder);
    let bind_values = sea_values_to_bind_values(values)?;

    // Fallback count query covers the zero-row edge case described above.
    let count_sql = if query.pagination.count == CountPolicy::Emit {
        let mut count_select = Query::select();
        count_select
            .expr(Func::count(SeaExpr::col(Asterisk)))
            .from(Entities::Table)
            .cond_where(condition);
        let (count_sql_str, _) = count_select.build(PostgresQueryBuilder);
        Some(count_sql_str)
    } else {
        None
    };

    Ok(BuiltQuery { sql, count_sql, bind_values })
}

/// Column name used for the window-function count attached to each row of a
/// paginated entity query when `count=true`.
///
/// The entity repository reads the count back out under this name, so the two
/// have to agree; it is a constant rather than a literal for exactly that
/// reason.
pub const TOTAL_COUNT_COLUMN: &str = "total_count";

/// Builds a parameterized SQL query to retrieve only entity IDs from the `entities` table.
///
/// Similar to [`build_query`] but selects `id` instead of `payload`, used for
/// operations that only need to identify matching entities.
pub fn build_ids_query(query: &CompiledEntityQuery, attribute_names: &ResolvedAttributeNames) -> Result<BuiltQuery<'static>, RenderError> {
    let condition = build_entity_conditions(query, attribute_names)?;

    let mut select = Query::select();
    select.column(Entities::Id).from(Entities::Table).cond_where(condition);

    // Add ORDER BY clause (default to id ASC if none specified)
    if query.ordering.terms.is_empty() {
        select.order_by(Entities::Id, Order::Asc);
    } else {
        add_order_clause(
            &mut select,
            &query.ordering.terms,
            query.selection.geo.as_ref(),
            query.ordering.geometry.as_ref(),
        )?;
    }

    select.limit(query.pagination.limit as u64).offset(query.pagination.offset as u64);

    let (sql, values) = select.build(PostgresQueryBuilder);
    let bind_values = sea_values_to_bind_values(values)?;

    Ok(BuiltQuery {
        sql,
        count_sql: None,
        bind_values,
    })
}

/// Builds sea-query `Condition` from compiled entity query filters.
fn build_entity_conditions(query: &CompiledEntityQuery, attribute_names: &ResolvedAttributeNames) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // ID filter: id = ANY($N)
    if !query.selection.ids.is_empty() {
        let ids: Vec<&str> = query.selection.ids.iter().map(AsRef::as_ref).collect();
        condition = condition.add(SeaExpr::col(Entities::Id).is_in(ids));
    }

    // ID pattern filter: id ~ $N
    if let Some(pattern) = &query.selection.id_pattern {
        condition = condition.add(regex_match(SeaExpr::col(Entities::Id).into(), SeaExpr::val(pattern.as_str()).into()));
    }

    if let Some(type_expr) = &query.selection.type_expr {
        condition = condition.add(type_condition(type_expr, TypesColumn::Entities));
    }

    // Attribute existence filter
    if !query.selection.attrs.is_empty() {
        let mut attr_cond = Condition::any();
        for attr in &query.selection.attrs {
            attr_cond = attr_cond.add(jsonb_key_exists(SeaExpr::cust("\"payload\"->0"), SeaExpr::val(attr.as_str()).into()));
        }
        condition = condition.add(attr_cond);
    }

    if let Some(q_expr) = &query.selection.q_expr {
        condition = condition.add(q_filter::compile(q_expr, QFilterTarget::Entities, attribute_names)?);
    }

    if let Some(geo) = &query.selection.geo {
        condition = condition.add(geo_sql::condition(
            geo,
            GeometrySource::EntityPayload { payload_column: "payload" },
            NullGeometryPolicy::Exclude,
        )?);
    }

    if let Some(scope_q) = &query.selection.scope_q {
        condition = condition.add(scope_condition(scope_q, ScopeTarget::CurrentPaths { column: "scope_paths" })?);
    }

    Ok(condition)
}

/// The SQL expression an `orderBy` term sorts on.
///
/// Clause 4.23 admits `id` and `type` alongside attribute names. Those are
/// columns of `entities`, not members of the payload, so they are read
/// directly; every other term addresses an attribute value inside the JSONB
/// payload.
fn order_value_expr(target: &OrderTarget, collation: Option<&IcuCollation>) -> String {
    let collate = collation.map(|c| format!(" COLLATE \"{}\"", c.as_str())).unwrap_or_default();
    match target {
        OrderTarget::Id => format!("\"id\"{collate}"),
        OrderTarget::Type => format!("\"types\"{collate}"),
        OrderTarget::Attribute(iri) => {
            let json_path = format!(
                "$[0].\"{}\"[0].\"{}\"[0].\"@value\"",
                escape_jsonpath_key(iri.as_str()),
                escape_jsonpath_key(VALUE.as_str())
            );
            format!("jsonb_path_query_first(\"payload\", '{}'){collate}", escape_jsonpath(&json_path))
        }
    }
}

/// Adds ORDER BY clause to select statement.
fn add_order_clause(
    select: &mut SelectStatement,
    order_by: &[CompiledOrderTerm],
    geo: Option<&GeoQuery>,
    ordering_geometry: Option<&OrderingGeometry>,
) -> Result<(), RenderError> {
    if order_by.is_empty() {
        return Ok(());
    }

    for term in order_by {
        let order = match term.direction {
            OrderDirection::Asc | OrderDirection::DistAsc => Order::Asc,
            OrderDirection::Desc | OrderDirection::DistDesc => Order::Desc,
        };

        if term.direction.is_distance() {
            // Distance-based ordering — reference geometry binds as EWKB.
            let ref_ewkb = match (ordering_geometry, geo) {
                (Some(og), _) => encode(&og.geometry)?,
                (None, Some(geo)) => encode(&geo.geometry)?,
                (None, None) => {
                    return Err(RenderError::Query(QueryError::DistanceOrderWithoutReference));
                }
            };
            let dist_expr: SimpleExpr = SeaExpr::cust_with_exprs(
                format!(
                    "ST_Distance({}, ST_MakeValid(ST_GeomFromEWKB($1))::geography)",
                    geo_extract_geography(&term.target)
                ),
                [SeaExpr::val(ref_ewkb.to_vec()).into()],
            );
            select.order_by_expr(dist_expr, order);
        } else {
            select.order_by_expr(SeaExpr::cust(order_value_expr(&term.target, term.collation.as_ref())), order);
        }
    }

    // Always add id ASC as tiebreaker
    select.order_by(Entities::Id, Order::Asc);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_domain::temporal::AttributeNameId;
    use andromeda_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_query::{
        entity::selection::EntitySelection,
        filter::{
            expression::Expr,
            parser,
            path::AttrPath,
            predicate::{CmpOp, Predicate},
            scalar::{CompareValue, Scalar},
        },
        pagination::CountPolicy,
        pattern::Pattern,
        scope::query::ScopeQuery,
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
    use andromeda_postgresql_query::bind::BindValue;
    use iri_rs::IriBuf;

    fn ids(raw: &[&str]) -> Vec<Uri> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// A GeoJSON point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: serde_json::Value) -> Geometry {
        Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
    }

    /// A query with every filter absent, for tests that vary one part at a time.
    fn empty_query() -> CompiledEntityQuery {
        CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build()
    }

    #[test]
    fn build_empty_query_returns_base_sql() {
        let q = empty_query();
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("SELECT"));
        assert!(built.sql.contains("\"payload\""));
        assert!(built.sql.contains("\"entities\""));
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
        assert!(built.count_sql.is_none());
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.selection.ids = ids(&["urn:a", "urn:b"]);
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("\"id\" IN"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.selection.id_pattern = Some(Pattern::parse("urn:ngsi-ld:Vehicle:.*").unwrap());
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("\"id\" ~"));
    }

    #[test]
    fn build_with_single_type() {
        let mut q = empty_query();
        q.selection.type_expr = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("\"types\""));
    }

    #[test]
    fn build_with_mixed_type_expr() {
        let mut q = empty_query();
        // (Home AND Vehicle) OR Motorhome
        q.selection.type_expr = Some(TypeExpr::Or(vec![
            TypeExpr::And(vec![
                TypeExpr::Type(iri("https://example.com/Home")),
                TypeExpr::Type(iri("https://example.com/Vehicle")),
            ]),
            TypeExpr::Type(iri("https://example.com/Motorhome")),
        ]));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("= ANY"));
        assert!(built.sql.contains("AND"));
        assert!(built.sql.contains("OR"));
    }

    #[test]
    fn build_with_count() {
        let mut q = empty_query();
        q.pagination.count = CountPolicy::Emit;
        q.selection.type_expr = Some(TypeExpr::Type(iri("https://example.com/Vehicle")));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.count_sql.is_some());
        assert!(built.count_sql.unwrap().contains("COUNT(*)"));
    }

    fn temperature_is_known() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([(
            "https://example.com/temperature".parse().expect("valid attribute name"),
            AttributeNameId::from(12),
        )])
    }

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.selection.q_expr = Some(parser::parse("\"https://example.com/temperature\"").unwrap());
        let built = build_query(&q, &temperature_is_known()).unwrap();
        // Existence resolves through the temporal catalogue: `payload->0 ? $1`
        // could never use the GIN index on `payload`.
        assert!(built.sql.contains("\"temporal_entity_attributes\" AS \"tea_q\""), "{}", built.sql);
        assert!(!built.sql.contains("\"payload\"->0 ?"), "{}", built.sql);
    }

    #[test]
    fn build_with_q_comparison() {
        let mut cq = empty_query();
        cq.selection.q_expr = Some(Expr::Pred(Predicate::Compare {
            path: AttrPath::simple("https://example.com/temperature".parse().expect("valid attribute name")),
            op: CmpOp::Greater,
            rhs: CompareValue::Scalar(Scalar::Number(50.into())),
        }));
        let built = build_query(&cq, &temperature_is_known()).unwrap();
        assert!(built.sql.contains("EXISTS"), "{}", built.sql);
        assert!(built.sql.contains("\"ac\".\"value_number\" >"), "{}", built.sql);
        assert!(
            !built.sql.contains("jsonb_path_query_first"),
            "value predicates no longer scan the payload: {}",
            built.sql
        );
    }

    #[test]
    fn count_sql_reuses_the_main_query_bind_vector() {
        // The repository binds one vector against both statements, so the
        // placeholder sequence has to be identical up to the pagination
        // parameters the count query omits.
        let mut cq = empty_query();
        cq.pagination.count = CountPolicy::Emit;
        cq.selection.q_expr = Some(Expr::Pred(Predicate::Compare {
            path: AttrPath::simple("https://example.com/temperature".parse().expect("valid attribute name")),
            op: CmpOp::Greater,
            rhs: CompareValue::Scalar(Scalar::Number(50.into())),
        }));
        let built = build_query(&cq, &temperature_is_known()).unwrap();
        let count_sql = built.count_sql.expect("count sql");
        let placeholders = |sql: &str| {
            let mut found: Vec<String> = Vec::new();
            let bytes = sql.as_bytes();
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'$' {
                    let digits: String = sql[index + 1..].chars().take_while(char::is_ascii_digit).collect();
                    if !digits.is_empty() {
                        found.push(digits);
                    }
                }
            }
            found
        };
        let main = placeholders(&built.sql);
        let counted = placeholders(&count_sql);
        assert_eq!(counted, main[..counted.len()], "count query must reuse the leading binds verbatim");
        assert_eq!(counted, vec!["1".to_string(), "2".to_string()], "attribute id then value: {count_sql}");
    }

    #[test]
    fn build_with_limit_offset() {
        let mut q = empty_query();
        q.pagination.limit = 50;
        q.pagination.offset = 100;
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("LIMIT"));
        assert!(built.sql.contains("OFFSET"));
        // sea-query parameterizes limit/offset as bind values
        assert!(!built.bind_values.is_empty(), "should have bind values for limit/offset");
    }

    #[test]
    fn build_with_order_by_asc() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(iri("https://example.com/name")),
            direction: OrderDirection::Asc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ORDER BY"));
        assert!(built.sql.contains("ASC"));
        assert!(built.sql.contains("hasValue"));
    }

    #[test]
    fn build_with_order_by_collation() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(iri("https://example.com/name")),
            direction: OrderDirection::Asc,
            collation: Some(IcuCollation::parse("und-u-ks-identic").expect("valid collation")),
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("COLLATE \"und-u-ks-identic\""));
        assert!(built.sql.contains("ASC"));
    }

    #[test]
    fn build_with_order_by_dist_asc_with_geo() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: None,
            },
            geometry: point(serde_json::json!([13.4, 52.5])),
            geoproperty_iri: iri(LOCATION.as_str()),
        });
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(iri(LOCATION.as_str())),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC"));
    }

    #[test]
    fn build_with_order_by_dist_without_geo_returns_error() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(iri(LOCATION.as_str())),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let result = build_query(&q, &ResolvedAttributeNames::default());
        assert!(result.is_err());
    }

    #[test]
    fn build_with_order_by_dist_with_ordering_geometry() {
        let mut q = empty_query();
        let geometry = Geometry::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(iri(LOCATION.as_str())),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC"));
        assert!(built.sql.contains("ST_GeomFromEWKB"));
        assert!(
            built.bind_values.iter().any(|b| matches!(b, BindValue::Bytes(_))),
            "reference geometry must be bound as EWKB bytes, not a GeoJSON string"
        );
    }

    #[test]
    fn build_with_order_by_dist_ordering_geometry_takes_precedence() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000000.0),
                min_distance: None,
            },
            geometry: point(serde_json::json!([2.3522, 48.8566])),
            geoproperty_iri: iri(LOCATION.as_str()),
        });
        let geometry = Geometry::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(iri(LOCATION.as_str())),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        // Ordering geometry (Berlin) takes precedence over the geo query (Paris);
        // since reference geometries are now bound as EWKB bytes, verify that the
        // bind values carry exactly one raw geometry blob for the distance expression.
        assert!(built.sql.contains("ST_GeomFromEWKB"));
        assert!(built.bind_values.iter().any(|b| matches!(b, BindValue::Bytes(_))));
    }

    #[test]
    fn geo_default_location_includes_expiration_guard() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(5000.0),
                min_distance: None,
            },
            geometry: point(serde_json::json!([13.4, 52.5])),
            geoproperty_iri: iri(LOCATION.as_str()),
        });
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_DWithin"), "should contain spatial predicate");
        assert!(built.sql.contains("EXISTS"), "should contain EXISTS expiration guard");
        assert!(built.sql.contains("expiresAt"), "should reference expiresAt IRI");
        assert!(built.sql.contains("NOW()"), "should compare against NOW()");
    }

    #[test]
    fn georel_variants_bind_reference_geometry_as_ewkb() {
        // Each GeoRel variant must parameterise the reference geometry as
        // EWKB bytes; raw GeoJSON-in-SQL is disallowed in reference position.
        let variants = vec![
            GeoRel::Within,
            GeoRel::Contains,
            GeoRel::Intersects,
            GeoRel::Disjoint,
            GeoRel::Equals,
            GeoRel::Overlaps,
            GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: Some(10.0),
            },
        ];

        for georel in variants {
            let mut q = empty_query();
            q.selection.geo = Some(GeoQuery {
                georel: georel.clone(), // clone: the relation names the failing case in every assertion below
                geometry: point(serde_json::json!([1.0, 2.0])),
                geoproperty_iri: iri(LOCATION.as_str()),
            });
            let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
            assert!(built.sql.contains("ST_GeomFromEWKB"), "{georel:?} SQL missing ST_GeomFromEWKB");
            // Default-location path must not re-decode GeoJSON for the reference side.
            assert!(
                !built.sql.contains("ST_GeomFromGeoJSON"),
                "{georel:?} SQL still contains ST_GeomFromGeoJSON: {}",
                built.sql
            );
            assert!(
                built.bind_values.iter().any(|b| matches!(b, BindValue::Bytes(_))),
                "{georel:?} missing EWKB bytes bind"
            );
        }
    }

    #[test]
    fn custom_geoproperty_preserves_jsonb_geojson_extraction() {
        // Custom GeoProperties are stored inside the JSONB payload; the
        // column-side extraction is the documented exception.
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(serde_json::json!([1.0, 2.0])),
            geoproperty_iri: iri("https://example.com/customGeo"),
        });
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("ST_GeomFromGeoJSON"), "column-side JSONB extraction must remain");
        assert!(built.sql.contains("ST_GeomFromEWKB"), "reference side must still bind EWKB");
    }

    #[test]
    fn scope_complex_generates_mixed() {
        let mut q = empty_query();
        q.selection.scope_q = Some(ScopeQuery::parse("/A;/B,/C/#").expect("valid scope query"));
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains(" AND "), "SQL should contain AND");
        assert!(built.sql.contains(" OR "), "SQL should contain OR");
        assert!(built.sql.contains("::lquery"), "SQL should cast to lquery");
    }

    // -- SQL literal escaping ------------------------------------------------

    /// Number of `'` characters in a statement. Every literal contributes an
    /// opening and a closing quote, and a doubled apostrophe contributes two, so
    /// a balanced statement always has an even count. An unescaped apostrophe
    /// inside a literal makes it odd.
    fn apostrophe_count(sql: &str) -> usize {
        sql.matches('\'').count()
    }

    #[test]
    fn a_geoproperty_iri_containing_an_apostrophe_does_not_terminate_the_sql_literal() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(serde_json::json!([1.0, 2.0])),
            geoproperty_iri: iri("https://example.com/o'brien"),
        });
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(
            built.sql.contains("o''brien"),
            "apostrophe must be doubled inside the jsonpath literal: {}",
            built.sql
        );
        assert_eq!(apostrophe_count(&built.sql) % 2, 0, "unbalanced SQL literal: {}", built.sql);
    }

    #[test]
    fn a_near_geoproperty_iri_containing_an_apostrophe_does_not_terminate_the_sql_literal() {
        let mut q = empty_query();
        q.selection.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(500.0),
                min_distance: None,
            },
            geometry: point(serde_json::json!([1.0, 2.0])),
            geoproperty_iri: iri("https://example.com/o'brien"),
        });
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("o''brien"), "{}", built.sql);
        assert_eq!(apostrophe_count(&built.sql) % 2, 0, "unbalanced SQL literal: {}", built.sql);
    }

    #[test]
    fn an_order_by_attribute_iri_containing_an_apostrophe_does_not_terminate_the_sql_literal() {
        let mut q = empty_query();
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(iri("https://example.com/o'brien")),
            direction: OrderDirection::Asc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("o''brien"), "{}", built.sql);
        assert_eq!(apostrophe_count(&built.sql) % 2, 0, "unbalanced SQL literal: {}", built.sql);
    }

    #[test]
    fn a_distance_ordering_geoproperty_iri_containing_an_apostrophe_does_not_terminate_the_sql_literal() {
        let mut q = empty_query();
        let geometry = Geometry::from_geojson_value(&serde_json::json!({
            "type": "Point",
            "coordinates": [13.3986, 52.5547],
        }))
        .unwrap();
        q.ordering.geometry = Some(OrderingGeometry::new(geometry));
        q.ordering.terms = vec![CompiledOrderTerm {
            target: OrderTarget::Attribute(iri("https://example.com/o'brien")),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q, &ResolvedAttributeNames::default()).unwrap();
        assert!(built.sql.contains("o''brien"), "{}", built.sql);
        assert_eq!(apostrophe_count(&built.sql) % 2, 0, "unbalanced SQL literal: {}", built.sql);
    }
}
