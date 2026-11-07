//! The aggregated representation of the Temporal Evolution of an Entity (ETSI
//! GS CIM 009 clause 4.5.19, and the grouping extension).
//!
//! An aggregated attribute holds no instance: for every requested method it
//! holds one `[value, start, end]` triple per period, and, under the grouped
//! extension, those triples nested one map key per grouping term. Clause
//! 4.5.19 gives one aggregate value per attribute, `datasetId` and period, and
//! the store returns exactly that: one row per period of one attribute
//! instance, in period order, so each row becomes exactly one triple. A period
//! holding no Attribute instance has no row and so no triple: the first member
//! of a triple is "the value obtained after applying the aggregation method
//! over the period", a method over no instance obtains no value, and clause
//! 4.5.0 rules null out as a value. The example of annex C clause C.5.14
//! answers likewise only the periods holding instances.

mod row_triples;

use crate::{
    entity_type::EntityTypeValue,
    temporal::{GroupKeyValue, TemporalAggregationRow, aggregated_entity::row_triples::nested, temporal_document::MergeMode},
};
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    time::rendering::instant_text,
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Serialize, Serializer, ser::SerializeSeq};

/// The key one attribute instance is grouped under: its name plus the
/// `datasetId` that clause 4.5.5 uses to tell the instances of one attribute
/// apart.
type InstanceKey = (AttributeName, Option<Identifier>);

/// The value one period of one method aggregates to.
#[derive(Clone, Debug, PartialEq)]
pub enum AggregateValue {
    /// The method has no value over the period.
    Absent,
    /// An exact number, in the shape the stored values have.
    Number(JsonNumber),
    /// A count of observations or of distinct values.
    Count(i64),
    /// A string extremum, or the mean time of day of a `Time` (Table
    /// 4.5.19.1-2), in the lexical form its family defines.
    Text(String),
    /// A boolean extremum.
    Boolean(bool),
    /// A `DateTime` extremum, in the sub-second precision it was observed with.
    Instant(DateTime<Utc>),
}

/// One `[value, start, end]` triple of an aggregation period.
///
/// The bounds are derived from the client's `timeAt` and `aggrPeriodDuration`,
/// so one carrying milliseconds renders them, or the period it names would be
/// mislabelled.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregatedTriple {
    /// The aggregate over the period.
    pub value: AggregateValue,
    /// The start of the period.
    pub start: Option<DateTime<Utc>>,
    /// The end of the period.
    pub end: Option<DateTime<Utc>>,
}

/// What one method aggregates to: the triples, or the triples nested by group
/// key (the grouped extension).
#[derive(Clone, Debug, PartialEq)]
pub enum AggregatedValues {
    /// The triples of the standard format, in period order.
    Triples(Vec<AggregatedTriple>),
    /// The triples nested under one map key per grouping term.
    Grouped(IndexMap<GroupKeyValue, AggregatedValues>),
}

impl AggregatedValues {
    /// Every triple, at whatever depth the grouping nests it.
    fn triples(&self) -> Box<dyn Iterator<Item = &AggregatedTriple> + '_> {
        match self {
            AggregatedValues::Triples(triples) => Box::new(triples.iter()),
            AggregatedValues::Grouped(groups) => Box::new(groups.values().flat_map(AggregatedValues::triples)),
        }
    }

    /// Keep only the triples `admits` admits, at whatever depth.
    fn retain(&mut self, admits: &impl Fn(&AggregatedTriple) -> bool) {
        match self {
            AggregatedValues::Triples(triples) => triples.retain(admits),
            AggregatedValues::Grouped(groups) => groups.values_mut().for_each(|values| values.retain(admits)),
        }
    }

    /// Give every triple the bounds `start` and `end`, at whatever depth.
    fn close_to(&mut self, start: DateTime<Utc>, end: DateTime<Utc>) {
        match self {
            AggregatedValues::Triples(triples) => {
                for triple in triples {
                    triple.start = Some(start);
                    triple.end = Some(end);
                }
            }
            AggregatedValues::Grouped(groups) => groups.values_mut().for_each(|values| values.close_to(start, end)),
        }
    }
}

/// One method of one aggregated attribute and what it aggregates to.
#[derive(Clone, Debug, PartialEq)]
pub struct MethodAggregate {
    /// The method, whose clause 4.5.19 name is the member it is written under.
    pub method: AggrMethod,
    /// What it aggregates to.
    pub values: AggregatedValues,
}

/// One aggregated attribute instance: the family, the `datasetId` and the
/// requested methods.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregatedAttribute {
    /// The family of the attribute.
    pub family: AttributeType,
    /// The dataset the aggregate is of, absent for the default instance.
    pub dataset_id: Option<Identifier>,
    /// The requested methods, in the order requested.
    pub aggregates: Vec<MethodAggregate>,
}

/// An entity in its aggregated representation.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregatedTemporalEntity {
    /// The entity's identifier.
    pub id: Identifier,
    /// The entity's types.
    pub type_: EntityTypeValue,
    /// The aggregated attributes, an attribute holding one object per
    /// `datasetId`.
    pub attrs: IndexMap<AttributeName, Vec<AggregatedAttribute>>,
}

impl AggregatedTemporalEntity {
    /// The aggregated entity `rows` make of `id`, each attribute named as
    /// `name` names it.
    ///
    /// `grouping_depth` is how many grouping terms nest the triples of each
    /// method, none for the standard format.
    #[must_use]
    pub fn from_rows(
        id: Identifier,
        type_: EntityTypeValue,
        rows: &[TemporalAggregationRow],
        methods: &[AggrMethod],
        grouping_depth: usize,
        name: impl Fn(&AttributeName) -> AttributeName,
    ) -> AggregatedTemporalEntity {
        // The family is carried on the group rather than re-read from a row,
        // which is what removes the "no rows in this group" case an index
        // built by pushing can never actually produce.
        let mut groups: IndexMap<InstanceKey, (AttributeType, Vec<&TemporalAggregationRow>)> = IndexMap::with_capacity(rows.len().min(64));
        for row in rows {
            groups
                .entry((row.attribute_name.clone(), row.dataset_id.clone())) // clone: the index owns the key its borrowed rows are filed under
                .or_insert_with(|| (row.attribute_type, Vec::new()))
                .1
                .push(row);
        }
        let mut attrs: IndexMap<AttributeName, Vec<AggregatedAttribute>> = IndexMap::with_capacity(groups.len());
        for ((attribute, dataset_id), (family, bucket_rows)) in groups {
            let aggregates = methods
                .iter()
                .map(|method| MethodAggregate {
                    method: *method,
                    values: nested(&bucket_rows, *method, 0, grouping_depth),
                })
                .collect();
            attrs.entry(name(&attribute)).or_default().push(AggregatedAttribute {
                family,
                dataset_id,
                aggregates,
            });
        }
        AggregatedTemporalEntity { id, type_, attrs }
    }

    /// Whether the entity holds no aggregated attribute.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    /// The least recent start and the most recent end of any period the
    /// entity holds an aggregate of.
    #[must_use]
    pub fn period_span(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let mut triples = self.triples();
        let first = triples.next()?;
        let (start, end) = triples.fold((first.start, first.end), |(start, end), triple| {
            ([start, triple.start].into_iter().flatten().min(), [end, triple.end].into_iter().flatten().max())
        });
        start.zip(end)
    }

    /// Report `start` and `end` as the bounds of every period the entity holds
    /// an aggregate of, so a whole-range aggregation names the one range of
    /// its response.
    pub fn close_periods_to(&mut self, start: DateTime<Utc>, end: DateTime<Utc>) {
        for attribute in self.attrs.values_mut().flatten() {
            for aggregate in &mut attribute.aggregates {
                aggregate.values.close_to(start, end);
            }
        }
    }

    /// Keep only the periods both of whose bounds `keep` admits: the periods
    /// lying wholly on the admitted side of a bound.
    pub fn retain_periods(&mut self, keep: impl Fn(DateTime<Utc>) -> bool) {
        self.retain_triples(|triple| triple.start.is_none_or(&keep) && triple.end.is_none_or(&keep));
    }

    /// Keep only the periods lying wholly in `interval` (clause 4.11). An
    /// attribute instance left without a period is removed, and an attribute
    /// left without an instance with it, as an aggregation of the window
    /// answers no period for either.
    pub fn retain_periods_within(&mut self, interval: TimeInterval) {
        self.retain_triples(|triple| interval.holds_period(triple.start, triple.end));
        self.attrs.retain(|_, instances| {
            instances.retain(|instance| instance.aggregates.iter().any(|aggregate| aggregate.values.triples().next().is_some()));
            !instances.is_empty()
        });
    }

    /// Keep only the triples `admits` admits, of every method of every
    /// attribute.
    fn retain_triples(&mut self, admits: impl Fn(&AggregatedTriple) -> bool) {
        for attribute in self.attrs.values_mut().flatten() {
            for aggregate in &mut attribute.aggregates {
                aggregate.values.retain(&admits);
            }
        }
    }

    /// Every triple of every method of every attribute.
    fn triples(&self) -> impl Iterator<Item = &AggregatedTriple> {
        self.attrs
            .values()
            .flatten()
            .flat_map(|attribute| attribute.aggregates.iter())
            .flat_map(|aggregate| aggregate.values.triples())
    }

    /// Put `other`'s aggregates of the same entity beside this one, as `mode`
    /// says.
    ///
    /// Two aggregates of one period are no aggregate of their union, so an
    /// aggregate already held is never combined with another: an attribute
    /// only `other` holds joins whole, and an inclusive holder's instance of
    /// a `datasetId` this entity holds none of joins its attribute. The
    /// entity's identifier and types stay the first holder's.
    pub fn merge(&mut self, other: AggregatedTemporalEntity, mode: MergeMode) {
        for (name, received) in other.attrs {
            match (self.attrs.get_mut(&name), mode) {
                (Some(held), MergeMode::Inclusive) => {
                    for attribute in received {
                        if !held.iter().any(|known| known.dataset_id == attribute.dataset_id) {
                            held.push(attribute);
                        }
                    }
                }
                (Some(_), MergeMode::Auxiliary) => {}
                (None, MergeMode::Inclusive | MergeMode::Auxiliary) => {
                    self.attrs.insert(name, received);
                }
            }
        }
    }
}

// Hand-written: the triples are written as arrays, and the grouped values as a
// map keyed by the group value's own map key.
impl Serialize for AggregatedValues {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AggregatedValues::Triples(triples) => serializer.collect_seq(triples),
            AggregatedValues::Grouped(groups) => serializer.collect_map(groups.iter().map(|(key, values)| (key.as_map_key(), values))),
        }
    }
}

// Hand-written: a triple is an array of three elements.
impl Serialize for AggregatedTriple {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut triple = serializer.serialize_seq(Some(3))?;
        triple.serialize_element(&self.value)?;
        triple.serialize_element(&self.start.map(Instant))?;
        triple.serialize_element(&self.end.map(Instant))?;
        triple.end()
    }
}

// Hand-written: each variant is the JSON value its family writes.
impl Serialize for AggregateValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AggregateValue::Absent => serializer.serialize_none(),
            AggregateValue::Number(number) => number.serialize(serializer),
            AggregateValue::Count(count) => serializer.serialize_i64(*count),
            AggregateValue::Text(text) => serializer.serialize_str(text),
            AggregateValue::Boolean(flag) => serializer.serialize_bool(*flag),
            AggregateValue::Instant(instant) => Instant(*instant).serialize(serializer),
        }
    }
}

/// An instant spelled with exactly the sub-second precision it carries, which
/// keeps the milliseconds of a client's `timeAt` in the period a triple names.
struct Instant(DateTime<Utc>);

// Hand-written: the spelling is the lexicon's, not chrono's.
impl Serialize for Instant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&instant_text(self.0))
    }
}

#[cfg(test)]
mod tests;
