//! Whose instances decide the aggregation eligibility of a temporal read.

use crate::{
    error::TemporalError,
    outcome::TemporalEntityIdOutcome,
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            RecordedAttributeShape,
            RecordedValueKind,
            ShapeHolders,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        read_scope::{ScopeReach, TemporalReadScope, UnrecordedHistory},
    },
    stub_broker::{live_target, service_reading, uri},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    temporal_format::TemporalFormat,
    time_rel::TimeRel,
};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::PageWindow,
    split_filter::LeafTruths,
    temporal::{compiled::CompiledTemporalQuery, contributed_entities::ContributedEntities},
};
use andromeda_ngsi_ld_request::{error::ParameterError, query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// The shapes of one Entity's instances inside the request, per attribute.
type EntityShapes = HashMap<AttributeName, HashSet<RecordedAttributeShape>>;

/// A history whose Entities each hold their instances inside the request in
/// the shapes `held` names, and every one of them anywhere in the branch.
struct ShapedReader {
    /// The shapes each Entity's instances inside the request carry.
    held: HashMap<Identifier, EntityShapes>,
    /// The Entities a query selects.
    selected: Vec<Identifier>,
    /// How many reads among the instances of a request ran.
    held_reads: AtomicUsize,
}

impl ShapedReader {
    /// A history of `held`, whose queries select `selected`.
    fn new(held: impl IntoIterator<Item = (Identifier, EntityShapes)>, selected: &[Identifier]) -> Arc<ShapedReader> {
        Arc::new(ShapedReader {
            held: held.into_iter().collect(),
            selected: selected.to_vec(),
            held_reads: AtomicUsize::new(0),
        })
    }

    /// The shapes the Entities `holders` yields carry that `wanted` keeps.
    fn union_of<'a>(
        &self,
        holders: impl IntoIterator<Item = &'a Identifier>,
        wanted: impl Fn(&AttributeName, &RecordedAttributeShape) -> bool,
    ) -> EntityShapes {
        let mut union = EntityShapes::new();
        for shapes in holders.into_iter().filter_map(|holder| self.held.get(holder)) {
            for (attribute, carried) in shapes {
                let kept: HashSet<RecordedAttributeShape> = carried.iter().copied().filter(|shape| wanted(attribute, shape)).collect();
                if !kept.is_empty() {
                    // clone: the union is keyed by the name each Entity's shapes are.
                    union.entry(attribute.clone()).or_default().extend(kept);
                }
            }
        }
        union
    }
}

#[async_trait]
impl TemporalHistoryReader for ShapedReader {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            aggregation: AggregationPushdown::OnTheFly,
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn read_scope<'a>(&self, target: &'a ContextTarget, _reach: ScopeReach<'_>) -> Result<TemporalReadScope<'a>, TemporalError> {
        Ok(TemporalReadScope::new(target, UnrecordedHistory::Absent))
    }

    async fn entity_exists(&self, _scope: &TemporalReadScope<'_>, _entity_id: &Identifier) -> Result<bool, TemporalError> {
        Ok(false)
    }

    async fn entity_types(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError> {
        Ok(HashMap::new())
    }

    async fn query_entity_ids(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<TemporalEntityIdOutcome, TemporalError> {
        Ok(TemporalEntityIdOutcome {
            entity_ids: Vec::new(),
            total: None,
        })
    }

    async fn filter_leaf_truths(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        _entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, LeafTruths>, TemporalError> {
        panic!("no split query is read")
    }

    async fn in_effect_instances(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        _entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        panic!("no split query is read")
    }

    async fn query_entity_history(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        panic!("no instance history is read")
    }

    async fn query_entity_aggregation(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        panic!("no Entity is selected to aggregate")
    }

    async fn query_merged_entity_aggregation(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_id: &Identifier,
        _entity_types: &[EntityTypeName],
        _compiled: &CompiledTemporalQuery,
        _answered: &[TemporalHistoryRow],
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        panic!("no Entity is merged")
    }

    async fn recorded_attribute_shapes(&self, _scope: &TemporalReadScope<'_>, attributes: &[AttributeName]) -> Result<EntityShapes, TemporalError> {
        Ok(self.union_of(self.held.keys(), |attribute, _| attributes.contains(attribute)))
    }

    async fn held_attribute_shapes(
        &self,
        _scope: &TemporalReadScope<'_>,
        holders: ShapeHolders<'_>,
        _compiled: &CompiledTemporalQuery,
        shapes: &EntityShapes,
    ) -> Result<EntityShapes, TemporalError> {
        self.held_reads.fetch_add(1, Ordering::Relaxed);
        let holders: Vec<&Identifier> = match holders {
            ShapeHolders::Entity(entity_id) => vec![entity_id],
            ShapeHolders::Selected => self.selected.iter().collect(),
        };
        Ok(self.union_of(holders, |attribute, shape| shapes.get(attribute).is_some_and(|wanted| wanted.contains(shape))))
    }
}

fn load() -> AttributeName {
    "https://example.com/load".parse().expect("valid attribute name")
}

/// The one shape `kind` of a Property's value, held under `load`.
fn load_as(kind: RecordedValueKind) -> EntityShapes {
    HashMap::from([(
        load(),
        HashSet::from([RecordedAttributeShape {
            family: AttributeType::Property,
            value_kind: kind,
        }]),
    )])
}

/// `avg` of `load` over every instance after 2024.
fn average_load() -> TemporalQueryParameters {
    TemporalQueryParameters {
        timerel: Some(TimeRel::After),
        time_at: Some("2024-01-01T00:00:00Z".parse().expect("a valid instant")),
        format: Some(WrittenValue::Supported(TemporalFormat::AggregatedValues)),
        aggr_methods: Some(vec![AggrMethod::Avg]),
        attrs: Some(vec![load()]),
        ..TemporalQueryParameters::default()
    }
}

fn array_holder() -> Identifier {
    uri("urn:ngsi-ld:Truck:1")
}

fn string_holder() -> Identifier {
    uri("urn:ngsi-ld:Label:1")
}

/// One Entity holding `load` as JSON Arrays and another holding it as Strings.
fn two_holders(selected: &[Identifier]) -> Arc<ShapedReader> {
    ShapedReader::new(
        [
            (array_holder(), load_as(RecordedValueKind::Array)),
            (string_holder(), load_as(RecordedValueKind::String)),
        ],
        selected,
    )
}

/// Whether the error refuses `avg` of `load` as not eligible.
fn refuses_average_load(error: &TemporalError) -> bool {
    matches!(
        error,
        TemporalError::Request(ParameterError::AggregationIneligible {
            attribute,
            method: AggrMethod::Avg,
        }) if *attribute == load()
    )
}

async fn query(reader: Arc<ShapedReader>) -> Result<(), TemporalError> {
    service_reading(reader)
        .query(
            &live_target(),
            &average_load(),
            &PageWindow::offset(20, 0),
            CountPolicy::Omit,
            &ContextSpec::core_only(),
            ContributedEntities::default(),
        )
        .await
        .map(drop)
}

async fn retrieve(reader: Arc<ShapedReader>, entity_id: &Identifier) -> Result<(), TemporalError> {
    service_reading(reader)
        .get(&live_target(), entity_id, &average_load(), &ContextSpec::core_only())
        .await
        .map(drop)
}

/// Table 4.5.19.1-1 defines `avg` over the sizes of JSON Arrays, and clause
/// 5.7.4.4 judges the requested Attributes of the selected Entities: a String
/// another Entity holds under the same name refuses nothing.
#[tokio::test]
async fn a_query_selecting_the_array_holder_is_aggregated_whatever_another_entity_holds() {
    let reader = two_holders(&[array_holder()]);
    query(Arc::clone(&reader)).await.expect("avg over arrays is eligible");
    assert_eq!(reader.held_reads.load(Ordering::Relaxed), 1);
}

/// Table 4.5.19.1-1 marks `avg` N/A for a JSON String.
#[tokio::test]
async fn a_query_selecting_the_string_holder_is_refused() {
    let error = query(two_holders(&[array_holder(), string_holder()]))
        .await
        .expect_err("avg over strings is not eligible");
    assert!(refuses_average_load(&error), "{error:?}");
}

/// A retrieval judges the one Entity it names, so it passes eligibility and
/// answers that the Entity has no Temporal Evolution here.
#[tokio::test]
async fn a_retrieval_of_the_array_holder_is_not_refused_for_another_entitys_strings() {
    let error = retrieve(two_holders(&[]), &array_holder()).await.expect_err("the double holds no Evolution");
    assert!(matches!(error, TemporalError::NotFound(_)), "{error:?}");
}

#[tokio::test]
async fn a_retrieval_of_the_string_holder_is_refused() {
    let error = retrieve(two_holders(&[]), &string_holder())
        .await
        .expect_err("avg over strings is not eligible");
    assert!(refuses_average_load(&error), "{error:?}");
}

/// When every shape the attribute holds anywhere admits the methods, no
/// instance of the request is looked at.
#[tokio::test]
async fn shapes_admitting_every_method_read_no_instance() {
    let reader = ShapedReader::new([(array_holder(), load_as(RecordedValueKind::Array))], &[array_holder()]);
    query(Arc::clone(&reader)).await.expect("avg over arrays is eligible");
    assert_eq!(reader.held_reads.load(Ordering::Relaxed), 0);
}
