//! Temporal entity assembly: [`TemporalEntityRenderer`] assembles a temporal
//! entity representation from decoded history rows (NGSI-LD clause 6.3.10).

use crate::temporal::history_row::{TimeRange, neutral_row_to_expanded_instance};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_ngsi_ld_query::{
    projection::{compiled::Projection, term::ProjectionTerm},
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_json::{Value, json};

/// Temporal attribute instances paired with their effective observation time.
type TemporalAttributeInstances = Vec<(Option<DateTime<Utc>>, Value)>;
/// Stable attribute insertion order for the rendered temporal entity.
type TemporalAttrMap = IndexMap<AttributeName, TemporalAttributeInstances>;

/// Output of [`TemporalEntityRenderer`]: the assembled entity JSON together
/// with pagination metadata needed for Content-Range header construction.
#[derive(Debug)]
pub struct TemporalEntityBuildResult {
    /// The fully-assembled temporal entity JSON value (expanded form).
    pub entity: Value,
    /// Whether the result set was truncated by the system-applied lastN cap.
    pub truncated: bool,
    /// The observed time range across all attribute instances.
    pub time_range: Option<TimeRange>,
    /// The maximum partition total across all attributes (for Content-Range size).
    pub max_partition_total: Option<i64>,
}

/// Assembles a temporal entity representation from decoded history rows.
///
/// Groups attribute instances by attribute name, applies time range cutting
/// for unsynchronized attributes (NGSI-LD clause 6.3.10), and emits the
/// entity JSON along with truncation/pagination metadata.
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
}

impl<'a> TemporalEntityRenderer<'a> {
    /// Create a renderer for the given entity and row set.
    #[must_use]
    pub const fn new(
        entity_id: &'a Uri,
        entity_types: &'a [EntityTypeName],
        rows: &'a [TemporalHistoryRow],
        compiled: &'a CompiledTemporalQuery,
    ) -> TemporalEntityRenderer<'a> {
        TemporalEntityRenderer {
            entity_id,
            entity_types,
            rows,
            compiled,
        }
    }
}

impl TemporalEntityRenderer<'_> {
    /// Assemble the temporal entity and the pagination metadata around it.
    #[must_use]
    pub fn render(self) -> TemporalEntityBuildResult {
        let TemporalEntityRenderer {
            entity_id,
            entity_types,
            rows,
            compiled,
        } = self;

        let mut attrs: TemporalAttrMap = IndexMap::with_capacity(rows.len().min(64));
        let mut truncated = false;
        let mut min_time: Option<DateTime<Utc>> = None;
        let mut max_time: Option<DateTime<Utc>> = None;
        let mut max_partition_total: Option<i64> = None;

        for row in rows {
            let attr_name = row.attribute_name.clone(); // clone: the typed map owns names while history rows remain borrowed.

            if compiled.instances.last_n_origin.is_system_capped()
                && let Some(last_n) = compiled.instances.last_n
                && let Some(total) = row.partition_total
            {
                max_partition_total = Some(max_partition_total.map_or(total, |m: i64| m.max(total)));
                if total > i64::from(last_n) {
                    truncated = true;
                }
            }

            // Clause 6.3.10 cuts unsynchronized attributes on the time axis the
            // query selected, so the position of an instance is its instant on
            // exactly that axis (clause 4.11 defaults it to `observedAt`). An
            // instance that carries no instant on the selected axis has no
            // position and takes no part in the cut.
            //
            // The reified Scope has no position at all. Clause 4.5.6 names it
            // beside "each Property and Relationship" rather than among them,
            // and clause 4.18 makes a Scope valid "from the time it has been
            // set until the time it has been explicitly removed" rather than
            // observed at an instant inside the window — annex C clause C.5.16
            // renders one set an hour before the window it answers. Letting
            // that instant into the cut would trim the attributes around it,
            // and letting it into the range would report a window the request
            // never asked for.
            let obs = if is_scope_attribute(&attr_name) {
                None
            } else {
                row.time_axis(compiled.window.timeproperty)
            };
            if let Some(t) = obs {
                min_time = Some(min_time.map_or(t, |m: DateTime<Utc>| m.min(t)));
                max_time = Some(max_time.map_or(t, |m: DateTime<Utc>| m.max(t)));
            }

            let instance = neutral_row_to_expanded_instance(row, compiled.representation.sys_attrs);
            attrs.entry(attr_name).or_default().push((obs, instance));
        }

        if truncated && attrs.len() > 1 {
            let attr_ranges: Vec<(&AttributeName, DateTime<Utc>, DateTime<Utc>)> = attrs
                .iter()
                .filter_map(|(name, instances)| {
                    let times: Vec<DateTime<Utc>> = instances.iter().filter_map(|(t, _)| *t).collect();
                    let min = times.iter().min().copied()?;
                    let max = times.iter().max().copied()?;
                    Some((name, min, max))
                })
                .collect();

            if attr_ranges.len() > 1 {
                let global_min = attr_ranges.iter().map(|(_, min, _)| *min).min();
                let global_max = attr_ranges.iter().map(|(_, _, max)| *max).max();
                if let (Some(g_min), Some(g_max)) = (global_min, global_max) {
                    let has_gap = attr_ranges.iter().any(|(_, a_min, _)| attr_ranges.iter().any(|(_, _, b_max)| *b_max < *a_min));

                    if has_gap {
                        let cut_max = if compiled.instances.last_n_origin.is_user_specified() { g_max } else { g_min };
                        let keep_range = attr_ranges.iter().find(|(_, min, max)| *min <= cut_max && cut_max <= *max);

                        if let Some((_, keep_min, keep_max)) = keep_range {
                            let (keep_min, keep_max) = (*keep_min, *keep_max);
                            for (name, instances) in &mut attrs {
                                if is_scope_attribute(name) {
                                    continue;
                                }
                                instances.retain(|(t, _)| t.is_some_and(|t| t >= keep_min && t <= keep_max));
                            }
                            min_time = Some(keep_min);
                            max_time = Some(keep_max);
                        }
                    }
                }
            }
        }

        project_attrs(&mut attrs, &compiled.projection);

        let mut entity = serde_json::Map::with_capacity(attrs.len() + 2);
        entity.insert("@id".to_string(), json!(entity_id));
        entity.insert("@type".to_string(), json!(entity_types));

        for (attr_name, instances) in attrs {
            let values: Vec<Value> = instances.into_iter().map(|(_, v)| v).collect();
            entity.insert(attr_name.to_string(), Value::Array(values));
        }

        let time_range = min_time.zip(max_time);
        TemporalEntityBuildResult {
            entity: Value::Object(entity),
            truncated,
            time_range,
            max_partition_total,
        }
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
fn is_scope_attribute(attr_name: &AttributeName) -> bool {
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
        temporal::{instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::TimeZone;
    use iri_rs::IriBuf;
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

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled).render();

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

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled).render();

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

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled).render();

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

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled).render();

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

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled).render();

        let object = built.entity.as_object().expect("object");
        assert!(!object.contains_key(expanded_properties::SCOPE.as_str()));
    }

    /// An empty projection narrows nothing.
    #[test]
    fn an_empty_projection_leaves_every_attribute_in_place() {
        let rows = vec![row("speed", 0), row("fuel", 1)];
        let compiled = compiled(Projection::default());

        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), &rows, &compiled).render();

        let object = built.entity.as_object().expect("object");
        assert!(object.contains_key("https://example.com/speed"));
        assert!(object.contains_key("https://example.com/fuel"));
    }
}
