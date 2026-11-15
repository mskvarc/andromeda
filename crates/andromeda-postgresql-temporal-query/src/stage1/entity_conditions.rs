//! The entity-level conditions of stage 1 of a temporal query, over the
//! Evolutions under `e`: the selectors, the Entities an `EntityMap` fixes, the
//! value, geo and Scope filters, and whether the projection leaves a member.

use crate::{projected_members::projected_member_condition, stage1::evolution_walk::TypeTest, value_filters::value_filter_condition};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    entity_selection::{ConsideredEntities, EntitySelector},
    identifier_axis::IdentifierAxis,
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_postgresql_query::{
    error::RenderError,
    history_source::HistoryBranch,
    operator::{PatternIndexes, pattern as pattern_predicate},
    query_names::QueryNames,
    type_predicate::condition as type_condition,
};
use andromeda_postgresql_schema::current::{E, Entities};
use andromeda_postgresql_seaquery::{literal::false_literal, pg_array::is_any_of};
use andromeda_postgresql_stored_values::resolved_names::ResolvedNames;
use sea_query::{Condition, Expr as SeaExpr};

/// Whether the selectors' identifier patterns are tested on the Evolutions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PatternTest {
    /// Each pattern is tested.
    Tested,
    /// The Evolutions were read from the identifiers the pattern matches,
    /// each rechecked against it, so it is not tested again.
    Answered,
}

/// Build entity-level conditions for temporal queries, over the Evolutions
/// under the `e` alias.
pub(super) fn build_entity_conditions_for_temporal(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    type_test: TypeTest,
    pattern_test: PatternTest,
) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // Entity selectors, unioned (clause 5.2.33): `(sel1) OR (sel2) OR ...`,
    // each reached through the `e` alias stage 1 selects the Evolutions
    // under. Rendered only when every selector constrains, the reading
    // `EntitySelection::selectors_constrain` shares with the entity path, and
    // a walk that selected the types leaves only a lone selector's pattern,
    // which the walk's candidates may answer too.
    let selectors = &compiled.entities.selectors;
    let untested_pattern = pattern_test == PatternTest::Answered || selectors.iter().all(|selector| selector.id_pattern.is_none());
    let nothing_left = type_test == TypeTest::Walked && untested_pattern;
    if compiled.entities.selectors_constrain() && !nothing_left {
        // A selector left with nothing to test admits every Evolution the
        // walk yields, and so does the union holding it.
        let tests: Option<Vec<Condition>> = selectors
            .iter()
            .map(|selector| Some(temporal_selector_condition(selector, &names.types, type_test, pattern_test)).filter(|test| !test.is_empty()))
            .collect();
        if let Some(tests) = tests {
            condition = condition.add(tests.into_iter().fold(Condition::any(), Condition::add));
        }
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
    if let Some(members) = projected_member_condition(branch, compiled, names) {
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
fn temporal_selector_condition(selector: &EntitySelector, types: &ResolvedNames<EntityTypeName>, type_test: TypeTest, pattern_test: PatternTest) -> Condition {
    let mut condition = Condition::all();
    match (selector.identifier_axis(), pattern_test) {
        (IdentifierAxis::Ids(ids), PatternTest::Tested | PatternTest::Answered) => {
            condition = condition.add(is_any_of(SeaExpr::col((E, Entities::Id)), ids.iter().map(Identifier::to_string)));
        }
        (IdentifierAxis::Pattern(pattern), PatternTest::Tested) => {
            condition = condition.add(pattern_predicate(
                SeaExpr::col((E, Entities::Id)),
                pattern,
                PatternIndexes::TrigramsAndByteOrder,
            ));
        }
        (IdentifierAxis::Pattern(_), PatternTest::Answered) | (IdentifierAxis::Any, PatternTest::Tested | PatternTest::Answered) => {}
    }
    match (&selector.type_expr, type_test) {
        (Some(type_expr), TypeTest::TypeKeys) => {
            condition = condition.add(type_condition(type_expr, &SeaExpr::col((E, Entities::EvolutionTypeNameKeys)), types));
        }
        (Some(_), TypeTest::Walked) | (None, TypeTest::TypeKeys | TypeTest::Walked) => {}
    }
    condition
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{built, built_count, ids, instant, minimal_compiled, point, speed_and_address},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity_selection::EntitySelector,
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser},
        geo::query::GeoQuery,
        page_window::PageWindow,
        scope::query::ScopeQuery,
        temporal::{
            compiled::CompiledTemporalQuery,
            contributed_entities::{ContributedEntities, ContributedRendering},
            window::TimeInterval,
        },
    };
    use andromeda_postgresql_query::{history_source::HistoryBranch, query_names::QueryNames};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use sea_query::{PostgresQueryBuilder, Value};
    use std::str::FromStr;

    /// Seventy thousand identifiers are past the 65535 parameters one statement
    /// may bind; the selector binds them as the one array they are, beside the
    /// window's own values.
    #[test]
    fn an_identifier_list_past_the_parameter_ceiling_binds_as_one_array() {
        let raw: Vec<String> = (0..70_000).map(|index| format!("urn:ngsi-ld:Vehicle:{index}")).collect();
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&raw.iter().map(String::as_str).collect::<Vec<_>>())).build()];
        let (sql, values) = built(&compiled, &speed_and_address()).build(PostgresQueryBuilder);
        assert!(sql.contains(r#""e"."id" = ANY($"#), "{sql}");
        assert!(values.0.len() < 100, "{} parameters", values.0.len());
        assert!(
            values
                .0
                .iter()
                .any(|value| matches!(value, Value::Array(_, Some(elements)) if elements.len() == raw.len())),
            "the identifiers are one bound array"
        );
    }

    /// The attribute dictionary keying one `GeoProperty`, as 9.
    fn geoproperty_is_known(geoproperty: &str) -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs([(geoproperty.parse().expect("valid attribute name"), AttributeNameKey::from(9))]),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    #[test]
    fn stage1_includes_scope_q_filter() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid/Gardens").expect("valid scope query"));
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("entity-conditions-stage1-includes-scope-q-filter", statement);
    }

    /// Over the queue the Scope probe compares the identifier a queued Scope
    /// entry carries, joined to recorded rows through their identity row.
    #[test]
    fn scope_history_over_the_queue_correlates_on_the_identifier() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid").expect("valid scope query"));
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("entity-conditions-scope-history-over-the-queue-correlates-on-the-identifier", statement);
    }

    #[test]
    fn stage1_scope_q_with_or_and_semantics() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("(/A;/B),/C").expect("valid scope query"));
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("entity-conditions-stage1-scope-q-with-or-and-semantics", statement);
    }

    #[test]
    fn temporal_scope_q_between() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Between {
            from: instant("2024-01-01T00:00:00Z"),
            to: instant("2024-06-01T00:00:00Z"),
        };
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("entity-conditions-temporal-scope-q-between", statement);
    }

    #[test]
    fn temporal_scope_q_compound() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("(/A;/B),/C").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Before(instant("2024-06-01T00:00:00Z"));
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("entity-conditions-temporal-scope-q-compound", statement);
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
        compiled.entities.q = Some(CompiledFilter::new(
            parser::parse("\"https://example.com/speed\">100").unwrap(),
            LanguageMatching::TagEquality,
        ));
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
    /// from them locally, and only from them: the identifier selector and the
    /// window still select it. The Scope query gates the page, whose full walk
    /// carries every filter.
    #[test]
    fn contributed_identifiers_exempt_only_the_value_filters() {
        let mut compiled = filtered_compiled();
        compiled.contributed = two_contributed();
        let page = build_stage1_entity_ids(
            &HistoryBranch::live(),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Omit,
            &speed_and_address(),
        )
        .expect("valid temporal query")
        .select;
        assert_statement_snapshot!("entity-conditions-contributed-identifiers-exempt-only-the-value-filters", page);
    }

    /// Without a contributed entity the statement is exactly the one a query
    /// that was never split renders.
    #[test]
    fn no_contributed_identifier_leaves_the_statement_as_it_was() {
        let compiled = filtered_compiled();
        let statement = built(&compiled, &speed_and_address());
        assert_statement_snapshot!("entity-conditions-no-contributed-identifier-leaves-the-statement-as-it-was-1", statement);
        let mut contributed_elsewhere = filtered_compiled();
        contributed_elsewhere.contributed = ContributedEntities::default();
        let elsewhere = built(&contributed_elsewhere, &speed_and_address());
        assert_statement_snapshot!("entity-conditions-no-contributed-identifier-leaves-the-statement-as-it-was-2", elsewhere);
        pretty_assertions::assert_eq!(elsewhere, statement);
    }

    /// Clause 5.7.4.4 S2: a temporal `q` "shall be checked against all the
    /// Attribute instances resulting from the initial filtering performed by the
    /// temporal query", so each Query Term probes the in-window history rows of
    /// its attribute, correlated to the temporal entity, and never current state.
    #[test]
    fn stage1_q_filter_probes_the_in_window_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
        compiled.entities.q = Some(CompiledFilter::new(
            parser::parse("\"https://example.com/speed\">100").unwrap(),
            LanguageMatching::TagEquality,
        ));
        let statement = built(&compiled, &speed_and_address());
        assert_statement_snapshot!("entity-conditions-stage1-q-filter-probes-the-in-window-history-instances", statement);
    }

    /// Clause 5.5.9: the count totals the same entities the page selects, so
    /// it carries both of the `q` probes, the instance in effect when the
    /// window opens included (clause 4.11, Annex C clause C.5.5).
    #[test]
    fn the_count_carries_the_same_two_probes() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
        compiled.entities.q = Some(CompiledFilter::new(
            parser::parse("\"https://example.com/speed\">100").unwrap(),
            LanguageMatching::TagEquality,
        ));
        let count = built_count(&compiled, &speed_and_address());
        assert_statement_snapshot!("entity-conditions-the-count-carries-the-same-two-probes", count);
    }

    /// The window a `q` term reads is on the requested Temporal Property.
    #[test]
    fn stage1_q_filter_reads_the_window_on_the_requested_timeproperty() {
        let mut compiled = minimal_compiled();
        compiled.window.timeproperty = TimeProp::Modified;
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
        compiled.entities.q = Some(CompiledFilter::new(
            parser::parse("\"https://example.com/speed\"==1").unwrap(),
            LanguageMatching::TagEquality,
        ));
        let statement = built(&compiled, &speed_and_address());
        assert_statement_snapshot!("entity-conditions-stage1-q-filter-reads-the-window-on-the-requested-timeproperty", statement);
    }

    /// The Query Terms of an OR share one windowed existential, each arm
    /// pairing its own attribute key with its own test: an instance satisfying
    /// either term is what the disjunction asks for (clause 4.9).
    #[test]
    fn the_terms_of_an_or_share_one_windowed_existential() {
        let mut compiled = minimal_compiled();
        compiled.entities.q = Some(CompiledFilter::new(
            parser::parse("\"https://example.com/speed\">100|\"https://example.com/speed\"<10").unwrap(),
            LanguageMatching::TagEquality,
        ));
        let statement = built(&compiled, &speed_and_address());
        assert_statement_snapshot!("entity-conditions-the-terms-of-an-or-share-one-windowed-existential", statement);
    }

    #[test]
    fn stage1_q_filter_on_an_unknown_attribute_is_unsatisfiable() {
        let mut compiled = minimal_compiled();
        compiled.entities.q = Some(CompiledFilter::new(
            parser::parse("\"https://example.com/never-written\">50").unwrap(),
            LanguageMatching::TagEquality,
        ));
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("entity-conditions-stage1-q-filter-on-an-unknown-attribute-is-unsatisfiable", statement);
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
            ..QueryNames::default()
        }
    }

    /// Clause 4.9 reads a compound member as a `MemberExpression` of the target
    /// value, which a history row holds in `value_object` as current state
    /// does, so the member is read off the stored value.
    #[test]
    fn a_q_predicate_on_a_compound_value_reads_the_value_object() {
        let mut compiled = minimal_compiled();
        compiled.entities.q = Some(CompiledFilter::new(
            parser::parse("\"https://example.com/address\"[city]==\"Berlin\"").unwrap(),
            LanguageMatching::TagEquality,
        ));
        let statement = built(&compiled, &speed_and_address());
        assert_statement_snapshot!("entity-conditions-a-q-predicate-on-a-compound-value-reads-the-value-object", statement);
    }

    /// Clause 5.7.4.4 S2 checks every shape of Query Term against the stored
    /// in-window instances, which hold their values and their sub-attributes
    /// in columns and elements, so no shape rebuilds an instance as a document.
    #[test]
    fn no_temporal_statement_rebuilds_a_history_instance_document() {
        let queries = [
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
        ];
        for (index, q) in queries.into_iter().enumerate() {
            let mut compiled = minimal_compiled();
            compiled.window.interval = TimeInterval::After(instant("2019-01-01T00:00:00Z"));
            compiled.entities.q = Some(CompiledFilter::new(parser::parse(q).expect("parse q"), LanguageMatching::TagEquality));
            let statement = built(&compiled, &speed_address_and_their_sub_attributes());
            let name = format!("entity-conditions-no-temporal-statement-rebuilds-a-history-instance-document-{}", index + 1);
            assert_statement_snapshot!(name, statement);
        }
    }

    /// Clause 5.7.4.4 S3: the geospatial restriction is checked against the
    /// `GeoProperty` instances "within the interval defined by the temporal
    /// query", which are `attribute_history` rows, not the single hoisted
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
        let statement = built(&compiled, &geoproperty_is_known("https://uri.etsi.org/ngsi-ld/location"));
        assert_statement_snapshot!("entity-conditions-a-default-location-geo-query-reads-the-windowed-history-instances", statement);
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
        let statement = built(&compiled, &geoproperty_is_known("https://uri.etsi.org/ngsi-ld/location"));
        assert_statement_snapshot!("entity-conditions-a-temporal-geo-query-restricts-the-geo-instances-to-the-window", statement);
    }

    #[test]
    fn no_scope_q_unchanged() {
        let compiled = minimal_compiled();
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("entity-conditions-no-scope-q-unchanged", statement);
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
        let statement = built(&compiled, &geoproperty_is_known("https://example.com/customGeo"));
        assert_statement_snapshot!(
            "entity-conditions-a-temporal-geo-query-on-a-named-geoproperty-reads-the-windowed-history-instances",
            statement
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
            ..QueryNames::default()
        };
        let statement = built(&compiled, &names);
        assert_statement_snapshot!("entity-conditions-a-temporal-geoproperty-iri-never-reaches-the-statement-text", statement);
    }
}
