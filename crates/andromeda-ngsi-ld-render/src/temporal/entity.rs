//! Temporal entity assembly: [`TemporalEntityRenderer`] assembles a temporal
//! entity representation from decoded history rows (NGSI-LD clause 6.3.10).

use crate::temporal::{
    history_row::{TimeRange, neutral_row_to_expanded_instance},
    page_cut::PageCut,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    temporal_format::TemporalFormat,
    time_prop::TimeProp,
    uri::Uri,
};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_ngsi_ld_query::{
    projection::{compiled::Projection, term::ProjectionTerm},
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_json::{Value, json};

/// Stable attribute insertion order for the rendered temporal entity.
type TemporalAttrMap = IndexMap<AttributeName, Vec<Value>>;

/// Output of [`TemporalEntityRenderer`]: the assembled entity JSON together
/// with the instants a `Content-Range` is built from.
#[derive(Debug)]
pub struct TemporalEntityBuildResult {
    /// The fully-assembled temporal entity JSON value (expanded form).
    pub entity: Value,
    /// The least and most recent instants the representation carries on the
    /// requested axis, Scope history aside.
    pub time_range: Option<TimeRange>,
    /// The family of every attribute the cut left without an instance, keyed
    /// by its expanded name: an empty instance array no longer says whether it
    /// held Properties or Relationships, which clause 4.5.9 still needs to name
    /// the attribute's `type` and member.
    pub emptied: IndexMap<AttributeName, AttributeType>,
}

/// Assembles a temporal entity representation from decoded history rows.
///
/// Groups attribute instances by attribute name, keeps only the instances on
/// the kept side of the response's common [`PageCut`] (clause 6.3.10), and
/// emits the entity JSON along with the instants it carries.
#[derive(Debug)]
pub struct TemporalEntityRenderer<'a> {
    /// Entity identifier emitted in the representation.
    entity_id: &'a Uri,
    /// Entity types emitted in the representation.
    entity_types: &'a [EntityTypeName],
    /// Neutral history observations to assemble.
    rows: &'a [TemporalHistoryRow],
    /// Compiled temporal representation policy.
    compiled: &'a CompiledTemporalQuery,
    /// The cut the partial response the entity belongs to is made at, absent
    /// when the response is complete.
    cut: Option<PageCut>,
}

impl<'a> TemporalEntityRenderer<'a> {
    /// Create a renderer for the given entity and row set, cut at `cut`.
    #[must_use]
    pub const fn new(
        entity_id: &'a Uri,
        entity_types: &'a [EntityTypeName],
        rows: &'a [TemporalHistoryRow],
        compiled: &'a CompiledTemporalQuery,
        cut: Option<PageCut>,
    ) -> TemporalEntityRenderer<'a> {
        TemporalEntityRenderer {
            entity_id,
            entity_types,
            rows,
            compiled,
            cut,
        }
    }
}

impl TemporalEntityRenderer<'_> {
    /// Assemble the temporal entity and the instants it carries.
    ///
    /// An attribute every instance of which lies beyond the cut is still
    /// rendered, with no instance: the stream was read, it merely continues on
    /// the next page, and a client reading the page by attribute finds it
    /// there (ETSI TP `020_14` and `021_15` read those empty attributes).
    #[must_use]
    pub fn render(self) -> TemporalEntityBuildResult {
        let TemporalEntityRenderer {
            entity_id,
            entity_types,
            rows,
            compiled,
            cut,
        } = self;
        let timeproperty = compiled.window.timeproperty;

        let mut attrs: TemporalAttrMap = IndexMap::with_capacity(rows.len().min(64));
        let mut min_time: Option<DateTime<Utc>> = None;
        let mut max_time: Option<DateTime<Utc>> = None;
        let timestamps = rendered_timestamps(compiled);

        for row in rows {
            // clone: the typed map owns names while history rows remain borrowed.
            let instances = attrs.entry(row.attribute_name.clone()).or_default();
            if cut.is_some_and(|cut| !cut.keeps(row, timeproperty)) {
                continue;
            }

            // The reified Scope has no position on the requested axis. Clause
            // 4.5.6 names it beside "each Property and Relationship" rather
            // than among them, and clause 4.18 makes a Scope valid "from the
            // time it has been set until the time it has been explicitly
            // removed" rather than observed at an instant inside the window --
            // annex C clause C.5.16 renders one set an hour before the window
            // it answers. Letting that instant into the range would report a
            // window the request never asked for.
            if !is_scope_attribute(&row.attribute_name)
                && let Some(instant) = row.time_axis(timeproperty)
            {
                min_time = Some(min_time.map_or(instant, |seen: DateTime<Utc>| seen.min(instant)));
                max_time = Some(max_time.map_or(instant, |seen: DateTime<Utc>| seen.max(instant)));
            }
            instances.push(neutral_row_to_expanded_instance(row, timestamps));
        }

        project_attrs(&mut attrs, &compiled.projection);

        let emptied = attrs
            .iter()
            .filter(|(_, instances)| instances.is_empty())
            .filter_map(|(attr_name, _)| {
                rows.iter()
                    .find(|row| &row.attribute_name == attr_name)
                    .map(|row| (attr_name.clone(), row.attribute_type)) // clone: the result outlives the map it is read from
            })
            .collect();

        let mut entity = serde_json::Map::with_capacity(attrs.len() + 2);
        entity.insert("@id".to_string(), json!(entity_id));
        entity.insert("@type".to_string(), json!(entity_types));

        for (attr_name, instances) in attrs {
            entity.insert(attr_name.to_string(), Value::Array(instances));
        }

        TemporalEntityBuildResult {
            entity: Value::Object(entity),
            time_range: min_time.zip(max_time),
            emptied,
        }
    }
}

/// Which clause 4.8 system timestamps the assembled instances carry.
///
/// `sysAttrs` decides it (clause 6.3.11), except that the clause 4.5.9
/// `temporalValues` pairs need the instant on the Temporal Property the query
/// ran on as their second element: over `createdAt` or `modifiedAt` that
/// instant is a system timestamp, so the instances keep them. The projection
/// that builds the pairs reads the value and that one instant alone, so the
/// other timestamps kept alongside never reach the response.
const fn rendered_timestamps(compiled: &CompiledTemporalQuery) -> SysAttrsInclusion {
    match (compiled.representation.format, compiled.window.timeproperty) {
        (TemporalFormat::TemporalValues, TimeProp::Created | TimeProp::Modified) => SysAttrsInclusion::Include,
        (TemporalFormat::TemporalValues, TimeProp::Observed | TimeProp::Deleted)
        | (TemporalFormat::Normalized | TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues, _) => compiled.representation.sys_attrs,
    }
}

/// Apply the compiled clause 4.21 `pick` / `omit` projection to a temporal
/// entity's attribute map.
///
/// `scope` has no separate struct field in a temporal representation -- it is
/// stored and rendered as an ordinary attribute-shaped row, keyed by its
/// expanded IRI like every other attribute -- so its half of the clause
/// 4.21 structural rule is applied here, against the
/// [`ProjectionTerm::Scope`] keyword, rather than by the id/type pruning that
/// runs later on the compacted representation. `id` and `type` are not
/// members of this map at all: they are inserted separately once this
/// function returns, and their half of the rule is applied on the compacted
/// value instead.
fn project_attrs(attrs: &mut TemporalAttrMap, projection: &Projection) {
    if !projection.pick.is_empty() {
        attrs.retain(|attr_name, _| {
            if is_scope_attribute(attr_name) {
                return projection.pick.contains(&ProjectionTerm::Scope);
            }
            projection.pick.iter().any(|term| matches_expanded_attribute(term, attr_name))
        });
    }

    if !projection.omit.is_empty() {
        attrs.retain(|attr_name, _| {
            if is_scope_attribute(attr_name) {
                return !projection.omit.contains(&ProjectionTerm::Scope);
            }
            // Clause 4.21: `omit=rel{a}` omits `a` inside the entity `rel`
            // links to, not `rel` itself. Only a bare omit term -- one
            // carrying no linked sub-projection -- removes the attribute from
            // this temporal representation.
            !projection
                .omit
                .iter()
                .filter(|term| matches_expanded_attribute(term, attr_name))
                .any(|term| term.linked_terms().is_empty())
        });
    }
}

/// Whether `attr_name` is the clause 4.21 `scope` keyword's attribute row.
pub(crate) fn is_scope_attribute(attr_name: &AttributeName) -> bool {
    attr_name.as_str() == expanded_properties::SCOPE.as_str()
}

/// Whether `term` names the attribute `attr_name`.
///
/// A history row's attribute name is already the attribute's expanded IRI --
/// history is never stored keyed by a compact term -- so only the expanded
/// half of [`ProjectionTerm::matches_attribute`] ever applies at this stage
/// of rendering.
fn matches_expanded_attribute(term: &ProjectionTerm, attr_name: &AttributeName) -> bool {
    match term {
        ProjectionTerm::Attribute { iri, .. } => iri.as_str() == attr_name.as_str(),
        ProjectionTerm::Id | ProjectionTerm::Type | ProjectionTerm::Scope => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::TemporalValue;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        pagination::Pagination,
        temporal::{instance_page::PageDirection, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::TimeZone;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    fn instant() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant")
    }

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn attr_name(name: &str) -> AttributeName {
        format!("https://example.com/{name}").parse().expect("valid attribute name")
    }

    fn row(attr: &str, instance: u32) -> TemporalHistoryRow {
        TemporalHistoryRow {
            attribute_name: attr_name(attr),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value: TemporalValue::Number(JsonNumber::try_from(1.0).expect("a finite number")),
            observed_at: Some(instant()),
            recorded_at: instant(),
            created_at: Some(instant()),
            modified_at: Some(instant()),
            instance: InstanceIdentity::Generated(Uuid::from_u128(u128::from(instance))),
            metadata: None,
            partition_total: None,
        }
    }

    fn compiled(projection: Projection) -> CompiledTemporalQuery {
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
            .pagination(Pagination::DEFAULT)
            .projection(projection)
            .build()
    }

    fn entity_id() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")
    }

    fn entity_types() -> Vec<EntityTypeName> {
        vec!["Vehicle".parse().expect("valid type name")]
    }

    /// Clause 4.21: `pick=speed` keeps only the `speed` attribute row and
    /// drops every other attribute this entity's history carries.
    #[test]
    fn pick_of_one_attribute_keeps_only_that_attribute() {
        let rows = vec![row("speed", 0), row("fuel", 1)];
        let compiled = compiled(Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("speed"),
                linked: Vec::new(),
            }],
            omit: Vec::new(),
        });

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled, None).render();

        let object = built.entity.as_object().expect("object");
        assert!(object.contains_key("https://example.com/speed"));
        assert!(!object.contains_key("https://example.com/fuel"));
        assert_eq!(object.get("@id"), Some(&json!(entity_id())));
    }

    /// Clause 4.21: `omit=speed` drops the named attribute and leaves every
    /// other attribute in place.
    #[test]
    fn omit_of_one_attribute_drops_only_that_attribute() {
        let rows = vec![row("speed", 0), row("fuel", 1)];
        let compiled = compiled(Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Attribute {
                iri: iri("speed"),
                linked: Vec::new(),
            }],
        });

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled, None).render();

        let object = built.entity.as_object().expect("object");
        assert!(!object.contains_key("https://example.com/speed"));
        assert!(object.contains_key("https://example.com/fuel"));
    }

    /// Clause 4.21: `pick=isParkedIn{category}` keeps the relationship
    /// attribute on the root -- the nested sub-projection narrows whatever it
    /// links to, which is a separate concern applied to the resolved Linked
    /// Entity, not to this root rendering.
    #[test]
    fn a_nested_pick_term_still_keeps_its_relationship_on_the_root() {
        let rows = vec![row("isParkedIn", 0), row("fuel", 1)];
        let compiled = compiled(Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("isParkedIn"),
                linked: vec![ProjectionTerm::Attribute {
                    iri: iri("category"),
                    linked: Vec::new(),
                }],
            }],
            omit: Vec::new(),
        });

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled, None).render();

        let object = built.entity.as_object().expect("object");
        assert!(object.contains_key("https://example.com/isParkedIn"));
        assert!(!object.contains_key("https://example.com/fuel"));
    }

    /// Clause 4.21: `omit=rel{a}` omits `a` inside whatever `rel` links to,
    /// not `rel` itself -- a nested omit term therefore keeps the
    /// relationship on the root exactly as an unrestricted attribute would.
    /// The narrowing this leaves for the linked entity itself is exercised in
    /// `andromeda-temporal`'s `a_nested_omit_narrows_the_resolved_target`.
    #[test]
    fn a_nested_omit_term_still_keeps_its_relationship_on_the_root() {
        let rows = vec![row("isParkedIn", 0), row("fuel", 1)];
        let compiled = compiled(Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Attribute {
                iri: iri("isParkedIn"),
                linked: vec![ProjectionTerm::Attribute {
                    iri: iri("category"),
                    linked: Vec::new(),
                }],
            }],
        });

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled, None).render();

        let object = built.entity.as_object().expect("object");
        assert!(
            object.contains_key("https://example.com/isParkedIn"),
            "a nested omit term must not remove the relationship itself"
        );
        assert!(
            object.contains_key("https://example.com/fuel"),
            "a nested omit term names no member of the root"
        );
    }

    /// A non-empty `pick` that does not name the `scope` keyword drops the
    /// `scope` attribute row, the same clause 4.21 structural rule the
    /// non-temporal API applies.
    #[test]
    fn a_pick_without_the_scope_keyword_drops_the_scope_attribute() {
        let mut scope_row = row("speed", 0);
        scope_row.attribute_name = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
        let rows = vec![row("speed", 1), scope_row];
        let compiled = compiled(Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("speed"),
                linked: Vec::new(),
            }],
            omit: Vec::new(),
        });

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled, None).render();

        let object = built.entity.as_object().expect("object");
        assert!(!object.contains_key(expanded_properties::SCOPE.as_str()));
    }

    /// An empty projection narrows nothing.
    #[test]
    fn an_empty_projection_leaves_every_attribute_in_place() {
        let rows = vec![row("speed", 0), row("fuel", 1)];
        let compiled = compiled(Projection::default());

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled, None).render();

        let object = built.entity.as_object().expect("object");
        assert!(object.contains_key("https://example.com/speed"));
        assert!(object.contains_key("https://example.com/fuel"));
    }

    /// A row observed `minutes` after the fixture instant.
    fn row_at(attr: &str, instance: u32, minutes: i64) -> TemporalHistoryRow {
        let mut row = row(attr, instance);
        row.observed_at = Some(instant() + chrono::Duration::minutes(minutes));
        row
    }

    /// Clause 6.3.10: the instances beyond the response's common cut are left
    /// out, an attribute left with none stays present and empty, and the range
    /// the representation reports is the one it actually carries.
    #[test]
    fn a_cut_leaves_out_the_instances_beyond_it_and_keeps_emptied_attributes() {
        let rows = vec![row_at("speed", 0, 1), row_at("speed", 1, 2), row_at("speed", 2, 3), row_at("fuel", 3, 5)];
        let cut = PageCut {
            direction: PageDirection::Forward,
            boundary: instant() + chrono::Duration::minutes(2),
        };
        let compiled = compiled(Projection::default());
        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled, Some(cut)).render();
        assert_eq!(built.entity["https://example.com/speed"].as_array().map(Vec::len), Some(2));
        assert_eq!(built.entity["https://example.com/fuel"], serde_json::json!([]));
        assert_eq!(
            built.time_range,
            Some((instant() + chrono::Duration::minutes(1), instant() + chrono::Duration::minutes(2)))
        );
        assert_eq!(
            built.emptied.into_iter().collect::<Vec<_>>(),
            vec![(attr_name("fuel"), AttributeType::Property)],
            "the emptied attribute keeps its family for clause 4.5.9"
        );
    }
}
