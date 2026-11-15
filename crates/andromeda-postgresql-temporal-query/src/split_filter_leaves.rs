//! The truth of every leaf of a temporal query's value, geo and scope filters
//! over the part of an entity this broker holds (clause 5.7.4.4
//! `splitEntities`).
//!
//! An entity a Context Source contributed a part of is judged on the union of
//! its parts. Every leaf of the filters is existential over the instances it
//! reads, so over the instances inside the window a leaf holds over the union
//! when it holds over any part, and the part held here is read with the probes
//! stage 1 conjoins, inside the window only: a Query Term reads the in-window
//! history, the geo-query the in-window `GeoProperty` instances, a scope
//! requirement the Scope events inside the window. The clause 4.11 instance in
//! effect when the window opens is the latest of its stream across every part,
//! which no part decides alone, so it is read apart and judged once the parts
//! are merged. A Query Term crossing a Relationship hop reads the linked
//! entities this broker holds; the settlement judges it again in memory over
//! the targets of every part.
//!
//! The leaves are read for a list of identifiers over the catalogue, so each
//! probe names the Evolution by its identifier.

use crate::value_filters::{scope_history_target, temporal_geo_existential, windowed_instances};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    split_filter::{FilterLeaf, SplitFilter},
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_postgresql_query::{
    error::RenderError,
    history_source::{HistoryBranch, HistoryOf, entity_rows::entity_rows},
    q_predicate::{instance_scan::InstanceScan, projection as q_filter},
    query_names::QueryNames,
    scope_predicate::pattern_condition,
    window_reach::WindowReach,
};
use andromeda_postgresql_schema::current::{E, Entities};
use andromeda_postgresql_seaquery::{literal::false_literal, pg_array::is_any_of};
use sea_query::{Alias, Expr as SeaExpr, SelectStatement};

/// Build the statement reading, for every entity of `entity_ids` this broker
/// holds a Temporal Evolution of, its identifier and then one boolean column
/// per leaf of the query's filters, in the order [`SplitFilter::leaves`]
/// yields them.
///
/// An identifier this broker holds no Temporal Evolution of has no row.
///
/// # Errors
///
/// Returns [`RenderError`] when the geo-query cannot be rendered, exactly as
/// stage 1 does.
pub fn build_filter_leaf_truths(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    entity_ids: &[Identifier],
    names: &QueryNames,
) -> Result<SelectStatement, RenderError> {
    let mut select = SelectStatement::new();
    select.column((E, Entities::Id)).from(entity_rows(branch, E));
    for (position, leaf) in SplitFilter::of(&compiled.entities).leaves().enumerate() {
        select.expr_as(leaf_expression(branch, compiled, leaf, names)?, Alias::new(format!("leaf_{position}")));
    }
    select.and_where(is_any_of(SeaExpr::col((E, Entities::Id)), entity_ids.iter().map(Identifier::to_string)));
    Ok(select.take())
}

/// One leaf rendered over the temporal entity `e` names.
fn leaf_expression(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, leaf: FilterLeaf<'_>, names: &QueryNames) -> Result<SeaExpr, RenderError> {
    let of = HistoryOf::Entity(SeaExpr::col((E, Entities::Id)));
    match leaf {
        FilterLeaf::Term(term) => {
            let window = windowed_instances(branch, compiled, &of);
            Ok(q_filter::compile_term(term, &InstanceScan::History(&window), names).into())
        }
        FilterLeaf::Geo(_) => Ok(temporal_geo_existential(branch, compiled, names, &of)?.map_or_else(false_literal, Into::into)),
        FilterLeaf::ScopeRequirement(pattern) => Ok(pattern_condition(pattern, &scope_history_target(branch, compiled, &of, WindowReach::Inside))),
    }
}

#[cfg(test)]
mod tests {
    use crate::split_filter_leaves::build_filter_leaf_truths;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser},
        geo::query::GeoQuery,
        scope::query::ScopeQuery,
        temporal::{
            compiled::CompiledTemporalQuery,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{history_source::HistoryBranch, query_names::QueryNames};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};

    fn compiled(q: &str, geo: Option<GeoQuery>, scope: Option<&str>) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(
                EntitySelection::builder()
                    .q(Some(CompiledFilter::new(parser::parse(q).expect("q parses"), LanguageMatching::TagEquality)))
                    .geo(geo)
                    .scope_q(scope.map(|raw| ScopeQuery::parse(raw).expect("scopeQ parses")))
                    .build(),
            )
            .window(
                TemporalWindow::builder()
                    .timeproperty(TimeProp::Observed)
                    .interval(TimeInterval::After("2019-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("instant").get()))
                    .build(),
            )
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn speed_known() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs([
                ("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameKey::from(7)),
                (
                    "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
                    AttributeNameKey::from(9),
                ),
            ]),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        }
    }

    fn ids() -> Vec<Identifier> {
        ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
            .iter()
            .map(|id| id.parse().expect("valid identifier"))
            .collect()
    }

    #[test]
    fn one_column_per_leaf_in_leaf_order() {
        let geo = GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        };
        let compiled = compiled("\"https://example.com/speed\">50", Some(geo), Some("/Madrid"));
        let statement = build_filter_leaf_truths(&HistoryBranch::live(), &compiled, &ids(), &speed_known()).expect("renders");
        assert_statement_snapshot!("split-filter-leaves-one-column-per-leaf-in-leaf-order", statement);
    }

    /// Clause 5.7.4.4 with clause 4.18: a local leaf reads the instances inside
    /// the window for `q` and the geo-query, and the Scope events inside it
    /// alone, the Scope in effect being the latest across every part.
    #[test]
    fn a_split_leaf_reads_inside_the_window_only() {
        let geo = GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        };
        let compiled = compiled("\"https://example.com/speed\">100", Some(geo), Some("/Madrid"));
        let statement = build_filter_leaf_truths(&HistoryBranch::live(), &compiled, &ids(), &speed_known()).expect("renders");
        assert_statement_snapshot!("split-filter-leaves-a-split-leaf-reads-inside-the-window-only", statement);
    }

    /// A hop leaf reads the local Relationship inside the window only, and
    /// under the `temporal_linked_retrieval` extension it lands on the linked
    /// entity's Temporal Evolution and reads its history over the same window.
    #[test]
    fn a_hop_leaf_reads_inside_the_window_and_the_linked_history() {
        let compiled = compiled("\"https://example.com/parkedIn\"{\"https://example.com/name\"}==\"Lot\"", None, None);
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([
                (
                    "https://example.com/parkedIn".parse().expect("valid attribute name"),
                    AttributeNameKey::from(11),
                ),
                ("https://example.com/name".parse().expect("valid attribute name"), AttributeNameKey::from(12)),
            ]),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        };
        let statement = build_filter_leaf_truths(&HistoryBranch::live(), &compiled, &ids(), &names).expect("renders");
        assert_statement_snapshot!("split-filter-leaves-a-hop-leaf-reads-inside-the-window-and-the-linked-history", statement);
    }

    /// Clause 5.7.4.4: the leaves of a split query judge the part held here,
    /// which on the live branch includes the queued entries, the entity source
    /// and every leaf probe alike.
    #[test]
    fn the_leaf_truths_read_the_union() {
        let compiled = compiled("\"https://example.com/speed\">50", None, Some("/Madrid"));
        let statement = build_filter_leaf_truths(&HistoryBranch::live(), &compiled, &ids(), &speed_known()).expect("renders");
        assert_statement_snapshot!("split-filter-leaves-the-leaf-truths-read-the-union", statement);
    }
}
