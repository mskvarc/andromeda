//! Temporal entity assembly: [`TemporalEntityRenderer`] assembles a temporal
//! entity representation from decoded history rows (NGSI-LD clause 6.3.10).

use crate::temporal::{
    history_row::{HistoryRowRenderError, TimeRange, neutral_row_to_instance},
    page_cut::PageCut,
};
use andromeda_linked_retrieval::{keyword::Keyword, narrowing::list::ListInForce};
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    temporal_format::TemporalFormat,
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_model::{
    entity_system_times::EntitySystemTimes,
    temporal::{TemporalHistoryRow, temporal_document::TemporalDocument},
};
use andromeda_ngsi_ld_query::{
    projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection},
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;

/// The instances of each attribute, in stable insertion order, with the family
/// the attribute's rows recorded.
type TemporalAttrMap = IndexMap<AttributeName, (AttributeType, Vec<InstanceDocument>)>;

/// Output of [`TemporalEntityRenderer`]: the assembled temporal entity together
/// with the instants a `Content-Range` is built from.
#[derive(Debug)]
pub struct TemporalEntityBuildResult {
    /// The assembled temporal entity: several instances per `datasetId`, each
    /// with its `instanceId` (clause 4.5.7), and the family of every attribute
    /// the cut left without an instance, which clause 4.5.9 still needs to name
    /// the attribute's `type` and member.
    pub document: TemporalDocument,
    /// The least and most recent instants the representation carries on the
    /// requested axis, Scope history aside.
    pub time_range: Option<TimeRange>,
}

/// Assembles a temporal entity representation from decoded history rows.
///
/// Groups attribute instances by attribute name, keeps only the instances on
/// the kept side of the response's common [`PageCut`] (clause 6.3.10), and
/// emits the entity along with the instants it carries.
#[derive(Debug)]
pub struct TemporalEntityRenderer<'a> {
    /// Entity identifier emitted in the representation.
    entity_id: &'a Identifier,
    /// Entity types emitted in the representation.
    entity_types: &'a [EntityTypeName],
    /// Neutral history observations to assemble, consumed by the assembly.
    rows: Vec<TemporalHistoryRow>,
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
        entity_id: &'a Identifier,
        entity_types: &'a [EntityTypeName],
        rows: Vec<TemporalHistoryRow>,
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
    /// there.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryRowRenderError`] for a stored row that no longer reads
    /// as the instance it recorded.
    pub fn render(self) -> Result<TemporalEntityBuildResult, HistoryRowRenderError> {
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
            let kept = cut.is_none_or(|cut| cut.keeps(&row, timeproperty));
            // The reified Scope has no position on the requested axis. Clause
            // 4.5.6 names it beside "each Property and Relationship" rather
            // than among them, and clause 4.18 makes a Scope valid "from the
            // time it has been set until the time it has been explicitly
            // removed" rather than observed at an instant inside the window --
            // annex C clause C.5.16 renders one set an hour before the window
            // it answers. Letting that instant into the range would report a
            // window the request never asked for.
            if kept
                && !is_scope_attribute(&row.attribute_name)
                && let Some(instant) = row.time_axis(timeproperty)
            {
                min_time = Some(min_time.map_or(instant, |seen: DateTime<Utc>| seen.min(instant)));
                max_time = Some(max_time.map_or(instant, |seen: DateTime<Utc>| seen.max(instant)));
            }
            let family = row.attribute_type;
            // clone: the attribute map owns the name the row is consumed with.
            let (_, instances) = attrs.entry(row.attribute_name.clone()).or_insert_with(|| (family, Vec::new()));
            if kept {
                instances.push(neutral_row_to_instance(row, timestamps)?);
            }
        }

        project_attrs(&mut attrs, &compiled.projection);

        let emptied = attrs
            .iter()
            .filter(|(_, (_, instances))| instances.is_empty())
            .map(|(attr_name, (family, _))| (attr_name.clone(), *family)) // clone: the result outlives the map it is read from
            .collect();

        let attributes = attrs
            .into_iter()
            .map(|(attr_name, (_, instances))| (attr_name, AttributeMember::Instances(instances)))
            .collect();
        let entity = EntityDocument::new(
            Some(entity_id.clone()), // clone: the document owns the identifier it names
            entity_types.to_vec(),
            None,
            None,
            attributes,
        );

        Ok(TemporalEntityBuildResult {
            document: TemporalDocument::new(entity, EntitySystemTimes::default(), emptied),
            time_range: min_time.zip(max_time),
        })
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
/// `scope` keyword, rather than by the id/type pruning that
/// runs later on the compacted representation. `id` and `type` are not
/// members of this map at all: they are inserted separately once this
/// function returns, and their half of the rule is applied on the compacted
/// value instead.
fn project_attrs(attrs: &mut TemporalAttrMap, projection: &Projection) {
    let list = ListInForce::of(Some(projection), &[]);
    if list.is_unrestricted() {
        return;
    }
    // A history row is keyed by the attribute's expanded IRI -- history is
    // never stored keyed by a compact term -- so the list matches it without
    // an index.
    let expanded = AttributeIriIndex::default();
    attrs.retain(|attr_name, _| {
        if is_scope_attribute(attr_name) {
            list.keeps_keyword(Keyword::Scope)
        } else {
            list.returns_attribute(attr_name.as_str(), &expanded)
        }
    });
}

/// Whether `attr_name` is the clause 4.21 `scope` keyword's attribute row.
pub(crate) fn is_scope_attribute(attr_name: &AttributeName) -> bool {
    attr_name.as_str() == expanded_properties::SCOPE.as_str()
}

#[cfg(test)]
mod tests {
    use crate::temporal::{entity::TemporalEntityRenderer, page_cut::PageCut};
    use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        entity_type_name::EntityTypeName,
        identifier::Identifier,
        instance_identity::InstanceIdentity,
        temporal_format::TemporalFormat,
        time_prop::TimeProp,
    };
    use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        projection::{compiled::Projection, term::ProjectionTerm},
        temporal::{
            compiled::CompiledTemporalQuery,
            instance_page::PageDirection,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::TemporalWindow,
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use chrono::{DateTime, Duration, TimeZone, Utc};
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
            value: TemporalValue::Number(JsonNumber::from(1)),
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
            .projection(projection)
            .build()
    }

    fn entity_id() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")
    }

    fn entity_types() -> Vec<EntityTypeName> {
        vec!["Vehicle".parse().expect("valid type name")]
    }

    fn attribute(name: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
        ProjectionTerm::Attribute { iri: iri(name), linked }
    }

    /// Assemble `rows` under `projection`, uncut.
    fn assembled(rows: Vec<TemporalHistoryRow>, projection: Projection) -> EntityDocument {
        let compiled = compiled(projection);
        TemporalEntityRenderer::new(&entity_id(), &entity_types(), rows, &compiled, None)
            .render()
            .expect("the rows render")
            .document
            .into_parts()
            .0
    }

    fn carries(entity: &EntityDocument, name: &str) -> bool {
        entity.attributes().contains_key(name)
    }

    /// The instances one attribute of the assembled entity carries.
    fn instances(entity: &EntityDocument, name: &str) -> Option<usize> {
        match entity.attributes().get(name)? {
            AttributeMember::Instances(instances) => Some(instances.len()),
            AttributeMember::Remove => None,
        }
    }

    /// Clause 4.21: `pick=speed` keeps only the `speed` attribute row and
    /// drops every other attribute this entity's history carries.
    #[test]
    fn pick_of_one_attribute_keeps_only_that_attribute() {
        let entity = assembled(
            vec![row("speed", 0), row("fuel", 1)],
            Projection {
                pick: vec![attribute("speed", Vec::new())],
                omit: Vec::new(),
            },
        );
        assert!(carries(&entity, "https://example.com/speed"));
        assert!(!carries(&entity, "https://example.com/fuel"));
        assert_eq!(entity.id(), Some(&entity_id()));
    }

    /// Clause 4.21: `omit=speed` drops the named attribute and leaves every
    /// other attribute in place.
    #[test]
    fn omit_of_one_attribute_drops_only_that_attribute() {
        let entity = assembled(
            vec![row("speed", 0), row("fuel", 1)],
            Projection {
                pick: Vec::new(),
                omit: vec![attribute("speed", Vec::new())],
            },
        );
        assert!(!carries(&entity, "https://example.com/speed"));
        assert!(carries(&entity, "https://example.com/fuel"));
    }

    /// Clause 4.21: `pick=isParkedIn{category}` keeps the relationship
    /// attribute on the root -- the nested sub-projection narrows whatever it
    /// links to, which is a separate concern applied to the resolved Linked
    /// Entity, not to this root rendering.
    #[test]
    fn a_nested_pick_term_still_keeps_its_relationship_on_the_root() {
        let entity = assembled(
            vec![row("isParkedIn", 0), row("fuel", 1)],
            Projection {
                pick: vec![attribute("isParkedIn", vec![attribute("category", Vec::new())])],
                omit: Vec::new(),
            },
        );
        assert!(carries(&entity, "https://example.com/isParkedIn"));
        assert!(!carries(&entity, "https://example.com/fuel"));
    }

    /// Clause 4.21: `omit=rel{a}` omits `a` inside whatever `rel` links to,
    /// not `rel` itself -- a nested omit term therefore keeps the
    /// relationship on the root exactly as an unrestricted attribute would.
    #[test]
    fn a_nested_omit_term_still_keeps_its_relationship_on_the_root() {
        let entity = assembled(
            vec![row("isParkedIn", 0), row("fuel", 1)],
            Projection {
                pick: Vec::new(),
                omit: vec![attribute("isParkedIn", vec![attribute("category", Vec::new())])],
            },
        );
        assert!(
            carries(&entity, "https://example.com/isParkedIn"),
            "a nested omit term must not remove the relationship itself"
        );
        assert!(carries(&entity, "https://example.com/fuel"), "a nested omit term names no member of the root");
    }

    /// A non-empty `pick` that does not name the `scope` keyword drops the
    /// `scope` attribute row, the same clause 4.21 structural rule the
    /// non-temporal API applies.
    #[test]
    fn a_pick_without_the_scope_keyword_drops_the_scope_attribute() {
        let mut scope_row = row("speed", 0);
        scope_row.attribute_name = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
        scope_row.value = TemporalValue::Scopes(vec!["/Madrid".parse().expect("a scope path")]);
        let entity = assembled(
            vec![row("speed", 1), scope_row],
            Projection {
                pick: vec![attribute("speed", Vec::new())],
                omit: Vec::new(),
            },
        );
        assert!(!carries(&entity, expanded_properties::SCOPE.as_str()));
    }

    /// An empty projection narrows nothing.
    #[test]
    fn an_empty_projection_leaves_every_attribute_in_place() {
        let entity = assembled(vec![row("speed", 0), row("fuel", 1)], Projection::default());
        assert!(carries(&entity, "https://example.com/speed"));
        assert!(carries(&entity, "https://example.com/fuel"));
    }

    /// A row observed `minutes` after the fixture instant.
    fn row_at(attr: &str, instance: u32, minutes: i64) -> TemporalHistoryRow {
        let mut row = row(attr, instance);
        row.observed_at = Some(instant() + Duration::minutes(minutes));
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
            boundary: instant() + Duration::minutes(2),
        };
        let compiled = compiled(Projection::default());
        let built = TemporalEntityRenderer::new(&entity_id(), &entity_types(), rows, &compiled, Some(cut))
            .render()
            .expect("the rows render");
        assert_eq!(instances(built.document.entity(), "https://example.com/speed"), Some(2));
        assert_eq!(instances(built.document.entity(), "https://example.com/fuel"), Some(0));
        assert_eq!(built.time_range, Some((instant() + Duration::minutes(1), instant() + Duration::minutes(2))));
        assert_eq!(
            built
                .document
                .emptied()
                .iter()
                .map(|(name, family)| (name.clone(), *family))
                .collect::<Vec<_>>(),
            vec![(attr_name("fuel"), AttributeType::Property)],
            "the emptied attribute keeps its family for clause 4.5.9"
        );
    }
}
