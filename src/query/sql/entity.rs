//! SQL generation for entity queries.

use crate::{
    error::AndromedaError,
    query::{
        compiled::{CompiledEntityQuery, CompiledOrderTerm, GeoQuery, OrderingGeometry},
        sql::{BindValue, BuiltQuery, EntityFilterConditionsInput, build_entity_filter_conditions, build_geo_extract},
    },
};

/// Builds a parameterized SQL query to retrieve entity payloads from the `entities` table.
///
/// Generates filter conditions from the compiled query (IDs, types, attrs, q expression,
/// geo filter, scope query), ordering, and pagination clauses.
pub fn build_query<'a>(query: &'a CompiledEntityQuery) -> Result<BuiltQuery<'a>, AndromedaError> {
    let mut binds: Vec<BindValue<'a>> = Vec::with_capacity(8);
    let mut param_idx = 1usize;

    let conditions = build_entity_filter_conditions(EntityFilterConditionsInput {
        ids: query.ids.as_deref(),
        id_pattern: query.id_pattern.as_ref().map(|p| p.as_str()),
        types: query.types.as_deref(),
        q_expr: query.q_expr.as_ref(),
        geo: query.geo.as_ref(),
        attrs: &query.attrs,
        scope_q: query.scope_q.as_ref().map(|s| s.as_str()),
        type_match_all: query.type_match_all,
        table_prefix: "",
        binds: &mut binds,
        param_idx: &mut param_idx,
    })?;

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let order_clause = build_order_clause(&query.order_by, query.geo.as_ref(), query.ordering_geometry.as_ref())?;
    let pagination = format!(" LIMIT {} OFFSET {}", query.limit, query.offset);

    let sql = format!("SELECT payload FROM entities{}{}{}", where_clause, order_clause, pagination);

    let count_sql = if query.count {
        Some(format!("SELECT COUNT(*) FROM entities{}", where_clause))
    } else {
        None
    };

    Ok(BuiltQuery {
        sql,
        count_sql,
        bind_values: binds,
    })
}

/// Builds the ORDER BY clause for entity queries.
///
/// Generates ordering expressions from compiled order terms, including support
/// for distance-based ordering (when a geo query is provided), collation, and
/// standard attribute-value ordering. Returns a complete ` ORDER BY ...` clause
/// string, or an empty string when no ordering terms are specified.
fn build_order_clause(order_by: &[CompiledOrderTerm], geo: Option<&GeoQuery>, ordering_geometry: Option<&OrderingGeometry>) -> Result<String, AndromedaError> {
    if order_by.is_empty() {
        return Ok(String::new());
    }

    let order_parts: Vec<String> = order_by
        .iter()
        .map(|term| {
            let dir = term.direction.sql_dir();
            if term.direction.is_distance() {
                // Distance-based ordering using ST_Distance with Haversine (geography type)
                // Three-tier precedence: orderFrom > geo query > error (S5.2.43)
                if let Some(og) = ordering_geometry {
                    // orderFrom takes precedence: use the ordering geometry as reference
                    let geo_extract = build_geo_extract(&term.attr_iri, "payload");
                    let geojson_str = serde_json::to_string(&og.geometry)
                        .map_err(|e| AndromedaError::bad_request_data(format!("Failed to serialize ordering geometry: {}", e)))?;
                    let ref_geom = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326))", geojson_str);
                    Ok(format!("ST_Distance({}::geography, {}::geography) {} NULLS LAST", geo_extract, ref_geom, dir))
                } else if let Some(geo) = geo {
                    // Fallback to geo query reference geometry
                    let geo_extract = build_geo_extract(&term.attr_iri, "payload");
                    let geojson = serde_json::json!({
                        "type": geo.geometry,
                        "coordinates": geo.coordinates
                    });
                    let ref_geom = format!("ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON('{}'), 4326))", geojson);
                    Ok(format!("ST_Distance({}::geography, {}::geography) {} NULLS LAST", geo_extract, ref_geom, dir))
                } else {
                    // Defensive: should have been caught during compilation (S5.7.2.4)
                    Err(AndromedaError::bad_request_data(
                        "Distance-based ordering requires either a geo-query or orderFrom parameter".to_string(),
                    ))
                }
            } else {
                let json_path = format!(
                    "$[0].\"{}\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\"",
                    term.attr_iri.replace('\\', "\\\\").replace('"', "\\\"")
                );
                let collation_clause = term.collation.as_ref().map(|c| format!(" COLLATE \"{}\"", c.as_str())).unwrap_or_default();
                Ok(format!(
                    "jsonb_path_query_first(payload, '{}'){} {} NULLS LAST",
                    json_path, collation_clause, dir
                ))
            }
        })
        .collect::<Result<Vec<_>, AndromedaError>>()?;
    Ok(format!(" ORDER BY {}, id ASC", order_parts.join(", ")))
}

/// Builds a parameterized SQL query to retrieve only entity IDs from the `entities` table.
///
/// Similar to [`build_query`] but selects `id` instead of `payload`, used for
/// operations that only need to identify matching entities.
pub fn build_ids_query<'a>(query: &'a CompiledEntityQuery) -> Result<BuiltQuery<'a>, AndromedaError> {
    let mut binds: Vec<BindValue<'a>> = Vec::with_capacity(8);
    let mut param_idx = 1usize;

    let conditions = build_entity_filter_conditions(EntityFilterConditionsInput {
        ids: query.ids.as_deref(),
        id_pattern: query.id_pattern.as_ref().map(|p| p.as_str()),
        types: query.types.as_deref(),
        q_expr: query.q_expr.as_ref(),
        geo: query.geo.as_ref(),
        attrs: &query.attrs,
        scope_q: query.scope_q.as_ref().map(|s| s.as_str()),
        type_match_all: query.type_match_all,
        table_prefix: "",
        binds: &mut binds,
        param_idx: &mut param_idx,
    })?;

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let order_clause = if query.order_by.is_empty() {
        " ORDER BY id ASC".to_string()
    } else {
        let base = build_order_clause(&query.order_by, query.geo.as_ref(), query.ordering_geometry.as_ref())?;
        if base.is_empty() { " ORDER BY id ASC".to_string() } else { base }
    };

    let pagination = format!(" LIMIT {} OFFSET {}", query.limit, query.offset);

    let sql = format!("SELECT id FROM entities{}{}{}", where_clause, order_clause, pagination);

    Ok(BuiltQuery {
        sql,
        count_sql: None,
        bind_values: binds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{
        compiled::{CompiledEntityQuery, CompiledOrderTerm, GeoQuery, GeoRel, IcuCollation, IdPattern, OrderDirection, OrderingGeometry, ScopeQuery},
        q::{
            ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar},
            parser,
        },
        sql::BindValue,
    };

    fn empty_query() -> CompiledEntityQuery {
        CompiledEntityQuery {
            ids: None,
            id_pattern: None,
            types: None,
            attrs: Vec::new(),
            q_expr: None,
            geo: None,
            ordering_geometry: None,
            limit: 20,
            offset: 0,
            count: false,
            pick: Vec::new(),
            omit: Vec::new(),
            order_by: Vec::new(),
            scope_q: None,
            type_match_all: false,
        }
    }

    #[test]
    fn build_empty_query_returns_base_sql() {
        let q = empty_query();
        let built = build_query(&q).unwrap();
        assert_eq!(built.sql, "SELECT payload FROM entities LIMIT 20 OFFSET 0");
        assert!(built.count_sql.is_none());
    }

    #[test]
    fn build_with_ids() {
        let mut q = empty_query();
        q.ids = Some(vec!["urn:a".to_string(), "urn:b".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("id = ANY($1)"));
    }

    #[test]
    fn build_with_id_pattern() {
        let mut q = empty_query();
        q.id_pattern = Some(IdPattern::new("urn:ngsi-ld:Vehicle:.*").unwrap());
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("id ~ $1"));
    }

    #[test]
    fn build_with_single_type() {
        let mut q = empty_query();
        q.types = Some(vec!["https://example.com/Vehicle".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("$1 = ANY(types)"));
    }

    #[test]
    fn build_with_multiple_types() {
        let mut q = empty_query();
        q.types = Some(vec!["TypeA".to_string(), "TypeB".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("types && $1"));
    }

    #[test]
    fn build_with_count() {
        let mut q = empty_query();
        q.count = true;
        q.types = Some(vec!["Vehicle".to_string()]);
        let built = build_query(&q).unwrap();
        assert!(built.count_sql.is_some());
        assert!(built.count_sql.unwrap().starts_with("SELECT COUNT(*)"));
    }

    #[test]
    fn build_with_q_existence() {
        let mut q = empty_query();
        q.q_expr = Some(parser::parse("temperature").unwrap());
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("payload->0 ? $1"));
    }

    #[test]
    fn build_with_q_comparison() {
        let mut cq = empty_query();
        let expr = Expr::Pred(Predicate::Compare {
            path: AttrPath::simple("https://example.com/temperature"),
            op: CmpOp::Greater,
            rhs: CompareValue::Scalar(Scalar::Number(50.into())),
        });
        cq.q_expr = Some(expr);
        let built = build_query(&cq).unwrap();
        assert!(built.sql.contains("jsonb_path_query_first"));
        assert!(built.sql.contains(">"));
    }

    #[test]
    fn build_with_limit_offset() {
        let mut q = empty_query();
        q.limit = 50;
        q.offset = 100;
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("LIMIT 50 OFFSET 100"));
    }

    #[test]
    fn build_with_order_by_asc() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://example.com/name".to_string(),
            direction: OrderDirection::Asc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ORDER BY"));
        assert!(built.sql.contains("ASC NULLS LAST"));
        assert!(built.sql.contains("hasValue"));
    }

    #[test]
    fn build_with_order_by_collation() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://example.com/name".to_string(),
            direction: OrderDirection::Asc,
            collation: Some(IcuCollation::new("und-u-ks-identic".to_string())),
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("COLLATE \"und-u-ks-identic\""));
        assert!(built.sql.contains("ASC NULLS LAST"));
    }

    #[test]
    fn build_with_order_by_dist_asc_with_geo() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([13.4, 52.5]),
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC NULLS LAST"));
    }

    #[test]
    fn build_with_order_by_dist_desc_with_geo() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(5000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([2.3, 48.8]),
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
            direction: OrderDirection::DistDesc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("DESC NULLS LAST"));
    }

    #[test]
    fn build_with_order_by_dist_without_geo_returns_error() {
        let mut q = empty_query();
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        // No geo query and no ordering geometry: must return error (S5.7.2.4)
        let result = build_query(&q);
        assert!(result.is_err());
    }

    #[test]
    fn build_with_order_by_dist_with_ordering_geometry() {
        let mut q = empty_query();
        q.ordering_geometry = Some(OrderingGeometry {
            geometry: geojson::Geometry::new(geojson::GeometryValue::Point {
                coordinates: vec![13.3986, 52.5547].into(),
            }),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        assert!(built.sql.contains("ASC NULLS LAST"));
        assert!(built.sql.contains("13.3986"));
    }

    #[test]
    fn build_with_order_by_dist_ordering_geometry_takes_precedence() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([2.3522, 48.8566]),
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
        });
        q.ordering_geometry = Some(OrderingGeometry {
            geometry: geojson::Geometry::new(geojson::GeometryValue::Point {
                coordinates: vec![13.3986, 52.5547].into(),
            }),
        });
        q.order_by = vec![CompiledOrderTerm {
            attr_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
            direction: OrderDirection::DistAsc,
            collation: None,
        }];
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Distance"));
        // Should use the ordering geometry (Berlin), not the geo query (Paris)
        assert!(built.sql.contains("13.3986"));
    }

    #[test]
    fn geo_default_location_includes_expiration_guard() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(5000.0),
                min_distance: None,
            },
            geometry: "Point".to_string(),
            coordinates: serde_json::json!([13.4, 52.5]),
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".to_string(),
        });
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_DWithin"), "should contain spatial predicate");
        assert!(built.sql.contains("EXISTS"), "should contain EXISTS expiration guard");
        assert!(built.sql.contains("expiresAt"), "should reference expiresAt IRI");
        assert!(built.sql.contains("NOW()"), "should compare against NOW()");
    }

    #[test]
    fn geo_non_default_geoproperty_omits_expiration_guard() {
        let mut q = empty_query();
        q.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: "Polygon".to_string(),
            coordinates: serde_json::json!([[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]]),
            geoproperty_iri: "https://example.com/customGeo".to_string(),
        });
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("ST_Within"), "should contain spatial predicate");
        assert!(!built.sql.contains("EXISTS"), "should NOT contain EXISTS guard for non-default geoproperty");
    }

    #[test]
    fn scope_exact_generates_lquery() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A/B".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("scope_paths ~ $"), "SQL should use ltree ~ lquery operator");
        assert!(built.sql.contains("::lquery"), "SQL should cast to lquery");
        match &built.bind_values[0] {
            BindValue::String(s) => assert_eq!(s.as_ref(), "A.B"),
            _other => panic!("expected String bind value"),
        }
    }

    #[test]
    fn scope_hash_generates_lquery() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A/#".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains("scope_paths ~ $"), "SQL should use ltree ~ lquery operator");
        match &built.bind_values[0] {
            BindValue::String(s) => assert_eq!(s.as_ref(), "A.*{1,}"),
            _other => panic!("expected String bind value"),
        }
    }

    #[test]
    fn scope_or_generates_disjunction() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A,/B".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains(" OR "), "SQL should contain OR for comma-separated scopes");
        let scope_count = built.sql.matches("scope_paths ~").count();
        assert_eq!(scope_count, 2, "should have two scope_paths ~ clauses");
    }

    #[test]
    fn scope_and_generates_conjunction() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A;/B".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains(" AND "), "SQL should contain AND for semicolon-separated scopes");
        let scope_count = built.sql.matches("scope_paths ~").count();
        assert_eq!(scope_count, 2, "should have two scope_paths ~ clauses");
    }

    #[test]
    fn scope_complex_generates_mixed() {
        let mut q = empty_query();
        q.scope_q = Some(ScopeQuery::new("/A;/B,/C/#".to_string()));
        let built = build_query(&q).unwrap();
        assert!(built.sql.contains(" AND "), "SQL should contain AND");
        assert!(built.sql.contains(" OR "), "SQL should contain OR");
        // Check that the hash wildcard is converted to lquery
        let has_hash_lquery = built.bind_values.iter().any(|b| matches!(b, BindValue::String(s) if s.as_ref() == "C.*{1,}"));
        assert!(has_hash_lquery, "should have C.*{{1,}} lquery bind value");
    }
}
