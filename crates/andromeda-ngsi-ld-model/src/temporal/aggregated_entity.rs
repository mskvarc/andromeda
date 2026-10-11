//! The aggregated representation of the Temporal Evolution of an Entity (ETSI
//! GS CIM 009 clause 4.5.19, and the grouping extension).
//!
//! An aggregated attribute holds no instance: for every requested method it
//! holds one `[value, start, end]` triple per period, and, under the grouped
//! extension, those triples nested one map key per grouping term. Clause
//! 4.5.19 gives one aggregate value per attribute, `datasetId` and period, and
//! the store returns exactly that: one row per period of one attribute
//! instance, in period order, so each row becomes exactly one triple. The
//! store has no row for a period holding no Attribute instance, while clause
//! 4.5.19 still gives that period its triple, so the query's period timeline
//! lays the empty periods out between the rows.

use crate::{
    entity_type::EntityTypeValue,
    temporal::{AggregationAverage, AggregationMinMax, GroupKeyValue, TemporalAggregationRow, temporal_document::MergeMode},
};
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    rfc3339::sub_second,
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::temporal::period_timeline::{PeriodLimitExceeded, PeriodTimeline, TimelinePeriod};
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

/// How the triples of each method are laid out.
#[derive(Clone, Copy, Debug)]
pub struct AggregationLayout<'t> {
    /// How many grouping terms nest the triples, none for the standard format.
    pub grouping_depth: usize,
    /// The periods of the query's time range, absent when the whole range is
    /// one period.
    pub timeline: Option<&'t PeriodTimeline>,
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
    /// method, none for the standard format. `timeline` lays out the periods
    /// holding no Attribute instance, absent when the whole range is one
    /// period.
    ///
    /// # Errors
    ///
    /// Returns [`PeriodLimitExceeded`] when the time range of one attribute
    /// divides into more periods than the timeline answers.
    pub fn from_rows(
        id: Identifier,
        type_: EntityTypeValue,
        rows: &[TemporalAggregationRow],
        methods: &[AggrMethod],
        layout: AggregationLayout<'_>,
        name: impl Fn(&AttributeName) -> AttributeName,
    ) -> Result<AggregatedTemporalEntity, PeriodLimitExceeded> {
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
                .map(|method| {
                    Ok(MethodAggregate {
                        method: *method,
                        values: nested(&bucket_rows, *method, 0, layout)?,
                    })
                })
                .collect::<Result<_, PeriodLimitExceeded>>()?;
            attrs.entry(name(&attribute)).or_default().push(AggregatedAttribute {
                family,
                dataset_id,
                aggregates,
            });
        }
        Ok(AggregatedTemporalEntity { id, type_, attrs })
    }

    /// Whether the entity holds no aggregated attribute.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
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

/// The values of one method over a set of rows, nested under one map key per
/// grouping term.
///
/// At each level the rows are partitioned by their group-key value at that
/// level, preserving the order the store returned them in (clause 4.23.2 plus
/// the query's collation, applied by the SQL `ORDER BY`). The leaf, once every
/// term has been consumed, is the triple array the standard format emits.
fn nested(rows: &[&TemporalAggregationRow], method: AggrMethod, level: usize, layout: AggregationLayout<'_>) -> Result<AggregatedValues, PeriodLimitExceeded> {
    if level == layout.grouping_depth {
        return Ok(AggregatedValues::Triples(triples(rows, method, layout.timeline)?));
    }
    let mut buckets: IndexMap<GroupKeyValue, Vec<&TemporalAggregationRow>> = IndexMap::new();
    for row in rows {
        let key = row.group_keys.get(level).cloned().unwrap_or(GroupKeyValue::Absent); // clone: the bucket map owns its keys
        buckets.entry(key).or_default().push(*row);
    }
    Ok(AggregatedValues::Grouped(
        buckets
            .into_iter()
            .map(|(key, sub_rows)| Ok((key, nested(&sub_rows, method, level + 1, layout)?)))
            .collect::<Result<_, PeriodLimitExceeded>>()?,
    ))
}

/// The triples one method contributes over the rows of one attribute
/// instance, in period order, with every empty period of the timeline in
/// place.
fn triples(rows: &[&TemporalAggregationRow], method: AggrMethod, timeline: Option<&PeriodTimeline>) -> Result<Vec<AggregatedTriple>, PeriodLimitExceeded> {
    let Some(timeline) = timeline else {
        return Ok(rows.iter().map(|row| triple(row, method)).collect());
    };
    Ok(timeline
        .lay_out(rows, |row| row.bucket_start.zip(row.bucket_end))?
        .into_iter()
        .map(|period| match period {
            TimelinePeriod::Held(row) => triple(row, method),
            TimelinePeriod::Empty { start, end } => AggregatedTriple {
                value: empty_period_value(method),
                start: Some(start),
                end: Some(end),
            },
        })
        .collect())
}

/// What one method aggregates to over a period holding no instance.
///
/// The counting methods count nothing, which is zero. Every other method of
/// Table 4.5.19.1-1 is calculated over "the values inside the period", and an
/// empty period has none to calculate over.
const fn empty_period_value(method: AggrMethod) -> AggregateValue {
    match method {
        AggrMethod::TotalCount | AggrMethod::DistinctCount => AggregateValue::Count(0),
        AggrMethod::Avg | AggrMethod::Min | AggrMethod::Max | AggrMethod::Sum | AggrMethod::SumSq | AggrMethod::StdDev => AggregateValue::Absent,
    }
}

/// The `[value, start, end]` triple one method contributes over one row.
fn triple(row: &TemporalAggregationRow, method: AggrMethod) -> AggregatedTriple {
    let value = match method {
        AggrMethod::TotalCount => row.total_count.map_or(AggregateValue::Absent, AggregateValue::Count),
        AggrMethod::DistinctCount => row.distinct_count.map_or(AggregateValue::Absent, AggregateValue::Count),
        AggrMethod::Min => row.min.as_ref().map_or(AggregateValue::Absent, extremum),
        AggrMethod::Max => row.max.as_ref().map_or(AggregateValue::Absent, extremum),
        AggrMethod::Avg => row.avg.as_ref().map_or(AggregateValue::Absent, average),
        AggrMethod::Sum => number(row.sum.as_ref()),
        AggrMethod::SumSq => number(row.sumsq.as_ref()),
        AggrMethod::StdDev => number(row.stddev.as_ref()),
    };
    AggregatedTriple {
        value,
        start: row.bucket_start,
        end: row.bucket_end,
    }
}

/// An exact number, or nothing.
fn number(value: Option<&JsonNumber>) -> AggregateValue {
    // clone: the aggregate owns the exact number the row holds
    value.map_or(AggregateValue::Absent, |number| AggregateValue::Number(number.clone()))
}

/// A typed minimum or maximum, in the family it was observed in.
///
/// The numeric extremum keeps its own JSON-number shape, so an integer beyond
/// 2^53 is the integer and not the double nearest to it.
fn extremum(value: &AggregationMinMax) -> AggregateValue {
    match value {
        AggregationMinMax::Number(number) => AggregateValue::Number(number.clone()), // clone: the aggregate owns the exact number the row holds
        AggregationMinMax::String(text) => AggregateValue::Text(text.clone()),       // clone: the aggregate owns the text the row holds
        AggregationMinMax::Boolean(flag) => AggregateValue::Boolean(*flag),
        AggregationMinMax::DateTime(instant) => AggregateValue::Instant(*instant),
    }
}

/// An arithmetic mean: Table 4.5.19.1-2 defines `avg` over a `Time` as "the
/// average time inside the period", so it is in the clause 4.6.3 `Time`
/// lexical form rather than the number of seconds it was computed from.
fn average(value: &AggregationAverage) -> AggregateValue {
    match value {
        AggregationAverage::Number(number) => AggregateValue::Number(number.clone()), // clone: the aggregate owns the exact number the row holds
        AggregationAverage::Time(time) => AggregateValue::Text(time.to_string()),
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
        serializer.serialize_str(&sub_second(self.0))
    }
}

#[cfg(test)]
mod tests;
