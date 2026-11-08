//! Whether the attributes of an aggregated temporal representation can be
//! aggregated by the methods requested (clause 5.7.4.4, Tables 4.5.19.1-1,
//! 4.5.19.1-2 and 4.5.19.1-3).
//!
//! Clause 5.7.4.4 judges the requested Attributes of the Entities the request
//! selects, over the instances the representation aggregates: those inside the
//! temporal window and of the datasets `datasetId` names. The judgement runs in
//! two reads. The shapes an attribute holds anywhere in the branch come first:
//! when every one admits every requested method, so does every subset of
//! them, and the request is eligible without reading any instance. Otherwise
//! only the shapes some method refuses are looked for among the instances the
//! representation aggregates.

use crate::port::history_reader::{RecordedAttributeShape, RecordedValueKind};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_request::error::ParameterError;
use std::{
    collections::{HashMap, HashSet},
    hash::BuildHasher,
};

/// The shapes of each attribute in `recorded` that refuse at least one of
/// `methods`; an attribute all of whose shapes admit every method is absent.
pub fn disqualifying_shapes<MapHasher, SetHasher>(
    methods: &[AggrMethod],
    recorded: &HashMap<AttributeName, HashSet<RecordedAttributeShape, SetHasher>, MapHasher>,
) -> HashMap<AttributeName, HashSet<RecordedAttributeShape>>
where
    MapHasher: BuildHasher,
    SetHasher: BuildHasher,
{
    recorded
        .iter()
        .filter_map(|(attribute, shapes)| {
            let refusing: HashSet<RecordedAttributeShape> = shapes
                .iter()
                .copied()
                .filter(|shape| methods.iter().any(|method| !admits(*shape, *method)))
                .collect();
            (!refusing.is_empty()).then(|| (attribute.clone(), refusing))
        })
        .collect()
}

/// Check every requested attribute against every requested method.
///
/// `recorded` holds the shapes each attribute has been recorded with anywhere
/// in the branch, and `held` those of its [`disqualifying_shapes`] that an
/// instance the representation aggregates carries. An attribute recorded
/// somewhere is eligible for a method when every shape `held` names for it
/// admits the method, so one holding none of them inside the request is. An
/// attribute never recorded has no values to aggregate and is eligible only for
/// the counting methods, which need neither a family nor a value shape at all.
///
/// # Errors
///
/// [`ParameterError::AggregationIneligible`] naming the first attribute and
/// method the tables do not admit.
pub fn ensure_aggregation_eligible<RecordedMap, RecordedSet, HeldMap, HeldSet>(
    attributes: &[AttributeName],
    methods: &[AggrMethod],
    recorded: &HashMap<AttributeName, HashSet<RecordedAttributeShape, RecordedSet>, RecordedMap>,
    held: &HashMap<AttributeName, HashSet<RecordedAttributeShape, HeldSet>, HeldMap>,
) -> Result<(), ParameterError>
where
    RecordedMap: BuildHasher,
    RecordedSet: BuildHasher,
    HeldMap: BuildHasher,
    HeldSet: BuildHasher,
{
    for attribute in attributes {
        for method in methods {
            let eligible = match recorded.get(attribute) {
                Some(shapes) if !shapes.is_empty() => held.get(attribute).is_none_or(|shapes| shapes.iter().all(|shape| admits(*shape, *method))),
                Some(_) | None => counting_only(*method),
            };
            if !eligible {
                return Err(ParameterError::AggregationIneligible {
                    attribute: attribute.clone(), // clone: the error owns the name it reports
                    method: *method,
                });
            }
        }
    }
    Ok(())
}

/// The two methods every column of every table defines, and the only ones a
/// family with no value-derived aggregate admits.
///
/// `distinctCount` and `totalCount` count instances rather than reading them,
/// so they carry no N/A cell anywhere in Tables 4.5.19.1-1, 4.5.19.1-2 and
/// 4.5.19.1-3.
const fn counting_only(method: AggrMethod) -> bool {
    match method {
        AggrMethod::DistinctCount | AggrMethod::TotalCount => true,
        AggrMethod::Avg | AggrMethod::Max | AggrMethod::Min | AggrMethod::StdDev | AggrMethod::Sum | AggrMethod::SumSq => false,
    }
}

/// Whether `method` is defined over an attribute recorded as `shape`.
///
/// Which of the three tables applies is decided by the attribute family, not by
/// the shape the broker happened to store the value in. Table 4.5.19.1-3 has a
/// single column and gives a Relationship `distinctCount` over its targets and
/// `totalCount` over its updates and nothing else, whatever those targets look
/// like in storage; only the Property families are read out of the
/// datatype-indexed Tables 4.5.19.1-1 and 4.5.19.1-2.
const fn admits(shape: RecordedAttributeShape, method: AggrMethod) -> bool {
    match shape.family {
        // Table 4.5.19.1-3, the whole of it: `avg`, `max`, `min`, `stddev`,
        // `sum` and `sumsq` are N/A for a Relationship, and clause 4.5.22.1
        // makes `ListRelationship` "a specialized type of Relationship" so the
        // same single-column table governs it: a multi-target `object` and an
        // `objectList` are both held as arrays of URIs, and neither may borrow
        // the array-size aggregates Table 4.5.19.1-1 gives a JSON Array. A
        // `VocabProperty` lands on the same two methods out of a different
        // table: clause 4.5.20.2 makes its `vocab` "a single string or array of
        // strings which can be type coerced into an IRI or array of IRIs", so
        // its values are URIs however many there are, and the "URI" column of
        // Table 4.5.19.1-2 marks every method but the counting ones N/A too.
        AttributeType::Relationship | AttributeType::ListRelationship | AttributeType::VocabProperty => counting_only(method),
        // The remaining families carry ordinary Property values, which Tables
        // 4.5.19.1-1 and 4.5.19.1-2 index by datatype.
        AttributeType::Property | AttributeType::GeoProperty | AttributeType::LanguageProperty | AttributeType::JsonProperty | AttributeType::ListProperty => {
            admits_value_shape(shape.value_kind, method)
        }
    }
}

/// Whether `method` is defined over Property values of shape `kind`.
///
/// One arm per column of Tables 4.5.19.1-1 (JSON native types) and 4.5.19.1-2
/// (`DateTime`, `Date`, `Time`, URI), each listing every method the column
/// defines and every method it marks N/A. The columns are transcribed rather
/// than generalised: `sum` over a JSON Array is the sum of the array sizes and
/// is defined, while `sumsq` over the same arrays is N/A, and `avg` over a
/// `Time` is defined while `avg` over the `Date` and `DateTime` beside it is
/// not.
const fn admits_value_shape(kind: RecordedValueKind, method: AggrMethod) -> bool {
    match kind {
        // Table 4.5.19.1-1, "JSON Number" column, and its "JSON Boolean"
        // column, whose note reads true as 1 and false as 0 so that every
        // arithmetic method carries over: both define all eight methods.
        RecordedValueKind::Number | RecordedValueKind::Boolean => match method {
            AggrMethod::Avg
            | AggrMethod::DistinctCount
            | AggrMethod::Max
            | AggrMethod::Min
            | AggrMethod::StdDev
            | AggrMethod::Sum
            | AggrMethod::SumSq
            | AggrMethod::TotalCount => true,
        },
        // Table 4.5.19.1-1, "JSON String" column, and Table 4.5.19.1-2's
        // "Date" and "DateTime" columns: ordering and counting, no arithmetic.
        // The three rows coincide because a string is ordered
        // lexicographically and a date chronologically, and the broker stores
        // both temporal forms in the canonical fixed-width rendering their
        // lexicon types define, in which the two orders are the same order.
        RecordedValueKind::String | RecordedValueKind::Date | RecordedValueKind::DateTime => match method {
            AggrMethod::DistinctCount | AggrMethod::Max | AggrMethod::Min | AggrMethod::TotalCount => true,
            AggrMethod::Avg | AggrMethod::StdDev | AggrMethod::Sum | AggrMethod::SumSq => false,
        },
        // Table 4.5.19.1-2, "Time" column: the average time inside the period
        // is defined - the clause's own example is the time a car enters a
        // parking - while `sum`, `sumsq` and `stddev` of a time of day are not.
        RecordedValueKind::Time => match method {
            AggrMethod::Avg | AggrMethod::DistinctCount | AggrMethod::Max | AggrMethod::Min | AggrMethod::TotalCount => true,
            AggrMethod::StdDev | AggrMethod::Sum | AggrMethod::SumSq => false,
        },
        // Table 4.5.19.1-1, "JSON Array" column: `avg`, `min`, `max` and `sum`
        // are taken over the sizes of the arrays inside the period, and only
        // `stddev` and `sumsq` are N/A.
        RecordedValueKind::Array => match method {
            AggrMethod::Avg | AggrMethod::DistinctCount | AggrMethod::Max | AggrMethod::Min | AggrMethod::Sum | AggrMethod::TotalCount => true,
            AggrMethod::StdDev | AggrMethod::SumSq => false,
        },
        // Table 4.5.19.1-1, "JSON Object" column, and Table 4.5.19.1-2's "URI"
        // column, which admit the two counting methods and nothing else. A
        // `GeoProperty` value is a `GeoJSON` object, so it is aggregated as the
        // JSON Object it is.
        RecordedValueKind::Object | RecordedValueKind::Geometry | RecordedValueKind::Uri => match method {
            AggrMethod::DistinctCount | AggrMethod::TotalCount => true,
            AggrMethod::Avg | AggrMethod::Max | AggrMethod::Min | AggrMethod::StdDev | AggrMethod::Sum | AggrMethod::SumSq => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        aggregation_eligibility::{disqualifying_shapes, ensure_aggregation_eligible},
        port::history_reader::{RecordedAttributeShape, RecordedValueKind},
    };
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType};
    use andromeda_ngsi_ld_request::error::ParameterError;
    use pretty_assertions::assert_eq;
    use std::collections::{HashMap, HashSet};

    /// Every method clause 4.5.19 defines, in the order the tables list them.
    const EVERY_METHOD: [AggrMethod; 8] = [
        AggrMethod::Avg,
        AggrMethod::DistinctCount,
        AggrMethod::Max,
        AggrMethod::Min,
        AggrMethod::StdDev,
        AggrMethod::Sum,
        AggrMethod::SumSq,
        AggrMethod::TotalCount,
    ];

    /// The two methods every column of every table defines.
    const COUNTING: [AggrMethod; 2] = [AggrMethod::DistinctCount, AggrMethod::TotalCount];

    fn attribute(name: &str) -> AttributeName {
        name.parse().expect("valid attribute name")
    }

    fn shape(family: AttributeType, value_kind: RecordedValueKind) -> RecordedAttributeShape {
        RecordedAttributeShape { family, value_kind }
    }

    fn recorded(entries: &[(&str, &[RecordedAttributeShape])]) -> HashMap<AttributeName, HashSet<RecordedAttributeShape>> {
        entries
            .iter()
            .map(|(name, shapes)| (attribute(name), shapes.iter().copied().collect()))
            .collect()
    }

    /// Assert that one recorded shape admits exactly `defined` and refuses
    /// every other method, so a table column is transcribed in full rather than
    /// only in the cells a change happened to touch.
    fn assert_column(shape: RecordedAttributeShape, defined: &[AggrMethod]) {
        let shapes = recorded(&[("attr", &[shape])]);
        for method in EVERY_METHOD {
            let expected_ok = defined.contains(&method);
            let actual = ensure_aggregation_eligible(&[attribute("attr")], &[method], &shapes, &shapes);
            assert_eq!(actual.is_ok(), expected_ok, "{shape:?} with {method}: {actual:?}");
        }
    }

    /// Table 4.5.19.1-1, "JSON Number" column: every method is defined over
    /// numbers.
    #[test]
    fn a_json_number_admits_every_aggregation_method() {
        assert_column(shape(AttributeType::Property, RecordedValueKind::Number), &EVERY_METHOD);
    }

    /// Table 4.5.19.1-1, "JSON Boolean" column with its note: true counts as 1
    /// and false as 0, so every method is defined over booleans too.
    #[test]
    fn a_json_boolean_admits_every_aggregation_method() {
        assert_column(shape(AttributeType::Property, RecordedValueKind::Boolean), &EVERY_METHOD);
    }

    /// Table 4.5.19.1-1, "JSON String" column: `min`, `max` and the counting
    /// methods are defined; `avg`, `stddev`, `sum` and `sumsq` are N/A.
    #[test]
    fn a_json_string_admits_ordering_and_counting_only() {
        assert_column(
            shape(AttributeType::Property, RecordedValueKind::String),
            &[AggrMethod::DistinctCount, AggrMethod::Max, AggrMethod::Min, AggrMethod::TotalCount],
        );
    }

    /// Table 4.5.19.1-1, "JSON Object" column: only the counting methods, and
    /// a `GeoProperty` geometry is the JSON Object it is rendered as.
    #[test]
    fn a_json_object_and_a_geometry_admit_counting_only() {
        assert_column(shape(AttributeType::Property, RecordedValueKind::Object), &COUNTING);
        assert_column(shape(AttributeType::GeoProperty, RecordedValueKind::Geometry), &COUNTING);
    }

    /// Table 4.5.19.1-1, "JSON Array" column: `avg`, `min`, `max` and `sum`
    /// are defined over the sizes of the arrays inside the period, and only
    /// `stddev` and `sumsq` are N/A.
    #[test]
    fn a_json_array_admits_every_method_except_stddev_and_sumsq() {
        assert_column(
            shape(AttributeType::Property, RecordedValueKind::Array),
            &[
                AggrMethod::Avg,
                AggrMethod::DistinctCount,
                AggrMethod::Max,
                AggrMethod::Min,
                AggrMethod::Sum,
                AggrMethod::TotalCount,
            ],
        );
    }

    /// Table 4.5.19.1-2, "`DateTime`" and "Date" columns: `min`, `max` and the
    /// counting methods, with `avg` explicitly N/A for both.
    #[test]
    fn a_date_time_and_a_date_admit_ordering_and_counting_but_never_avg() {
        for kind in [RecordedValueKind::DateTime, RecordedValueKind::Date] {
            assert_column(
                shape(AttributeType::Property, kind),
                &[AggrMethod::DistinctCount, AggrMethod::Max, AggrMethod::Min, AggrMethod::TotalCount],
            );
        }
    }

    /// Table 4.5.19.1-2, "Time" column: the average time inside the period is
    /// defined, unlike for the `Date` and `DateTime` beside it, while `sum`,
    /// `sumsq` and `stddev` stay N/A.
    #[test]
    fn a_time_admits_avg_which_the_other_temporal_types_refuse() {
        assert_column(
            shape(AttributeType::Property, RecordedValueKind::Time),
            &[
                AggrMethod::Avg,
                AggrMethod::DistinctCount,
                AggrMethod::Max,
                AggrMethod::Min,
                AggrMethod::TotalCount,
            ],
        );
    }

    /// Table 4.5.19.1-3: a Relationship admits `distinctCount` over its targets
    /// and `totalCount` over its updates, and every other method is N/A,
    /// whatever shape the broker stored the targets in. Clause 4.5.22.1 makes
    /// `ListRelationship` "a specialized type of Relationship", so the same row
    /// governs it, and a multi-target `object` or an `objectList` held as an
    /// array must not pick up the array-size aggregates of Table 4.5.19.1-1.
    #[test]
    fn table_4_5_19_1_3_gives_every_relationship_family_counting_alone() {
        for family in [AttributeType::Relationship, AttributeType::ListRelationship] {
            for kind in [RecordedValueKind::Uri, RecordedValueKind::Array] {
                assert_column(shape(family, kind), &COUNTING);
            }
        }
    }

    /// Table 4.5.19.1-2, "URI" column: a `VocabProperty` names vocabulary
    /// members by IRI (clause 4.5.20.2), one or several, so neither the single
    /// term stored as a string nor the array of them may be aggregated by any
    /// method but the two counting ones.
    #[test]
    fn table_4_5_19_1_2_gives_a_vocab_property_counting_alone() {
        for kind in [RecordedValueKind::String, RecordedValueKind::Array] {
            assert_column(shape(AttributeType::VocabProperty, kind), &COUNTING);
        }
    }

    /// An attribute recorded with several shapes is eligible only for the
    /// methods every shape admits; one never recorded only for counting.
    #[test]
    fn mixed_and_unrecorded_attributes_are_held_to_the_strictest_shape() {
        let shapes = recorded(&[(
            "mixed",
            &[
                shape(AttributeType::Property, RecordedValueKind::Number),
                shape(AttributeType::Property, RecordedValueKind::String),
            ],
        )]);
        assert!(ensure_aggregation_eligible(&[attribute("mixed")], &[AggrMethod::Max], &shapes, &shapes).is_ok());
        assert!(ensure_aggregation_eligible(&[attribute("mixed")], &[AggrMethod::Sum], &shapes, &shapes).is_err());
        assert!(ensure_aggregation_eligible(&[attribute("unknown")], &[AggrMethod::TotalCount], &shapes, &shapes).is_ok());
        assert!(ensure_aggregation_eligible(&[attribute("unknown")], &[AggrMethod::Avg], &shapes, &shapes).is_err());
    }

    /// An attribute written once as a Property carrying an array and once as a
    /// `ListRelationship` is held to the family that admits less: Table
    /// 4.5.19.1-3 refuses the array-size `avg` the Property column defines.
    #[test]
    fn an_attribute_recorded_in_two_families_is_held_to_the_stricter_table() {
        let shapes = recorded(&[(
            "targets",
            &[
                shape(AttributeType::Property, RecordedValueKind::Array),
                shape(AttributeType::ListRelationship, RecordedValueKind::Array),
            ],
        )]);
        assert!(ensure_aggregation_eligible(&[attribute("targets")], &[AggrMethod::Avg], &shapes, &shapes).is_err());
        assert!(ensure_aggregation_eligible(&[attribute("targets")], &[AggrMethod::TotalCount], &shapes, &shapes).is_ok());
    }

    /// The refusal names the attribute and the method the tables do not admit,
    /// so a client is told which pair of its request was rejected.
    #[test]
    fn the_refusal_names_the_attribute_and_the_method() {
        let shapes = recorded(&[("brandName", &[shape(AttributeType::Property, RecordedValueKind::String)])]);
        let error = ensure_aggregation_eligible(&[attribute("brandName")], &[AggrMethod::Sum], &shapes, &shapes).expect_err("strings cannot be summed");
        let reported = matches!(
            &error,
            ParameterError::AggregationIneligible {
                attribute: named,
                method: AggrMethod::Sum,
            } if *named == attribute("brandName")
        );
        assert!(reported, "expected an ineligible aggregation naming brandName and sum, got {error:?}");
    }

    /// Only the shapes some requested method refuses are looked for among the
    /// instances a request aggregates, and an attribute whose every shape
    /// admits every method is not looked for at all.
    #[test]
    fn the_disqualifying_shapes_are_those_some_requested_method_refuses() {
        let array = shape(AttributeType::Property, RecordedValueKind::Array);
        let text = shape(AttributeType::Property, RecordedValueKind::String);
        let number = shape(AttributeType::Property, RecordedValueKind::Number);
        let shapes = recorded(&[("load", &[array, text]), ("speed", &[number])]);
        assert_eq!(disqualifying_shapes(&[AggrMethod::Avg], &shapes), recorded(&[("load", &[text])]));
        assert_eq!(disqualifying_shapes(&[AggrMethod::Max, AggrMethod::TotalCount], &shapes), HashMap::new());
        assert_eq!(
            disqualifying_shapes(&[AggrMethod::StdDev], &shapes),
            recorded(&[("load", &[array, text])]),
            "Table 4.5.19.1-1 marks stddev N/A for a JSON Array too"
        );
    }

    /// Clause 5.7.4.4 judges the instances the request aggregates: a String
    /// recorded for the attribute elsewhere, by another Entity or outside the
    /// window, refuses nothing when none of those instances carries it.
    #[test]
    fn a_disqualifying_shape_held_outside_the_request_refuses_nothing() {
        let shapes = recorded(&[(
            "load",
            &[
                shape(AttributeType::Property, RecordedValueKind::Array),
                shape(AttributeType::Property, RecordedValueKind::String),
            ],
        )]);
        assert!(ensure_aggregation_eligible(&[attribute("load")], &[AggrMethod::Avg], &shapes, &recorded(&[])).is_ok());
    }

    /// The same String carried by an instance the request aggregates refuses
    /// `avg`, and still admits the methods its column defines.
    #[test]
    fn a_disqualifying_shape_held_inside_the_request_refuses_its_methods() {
        let text = shape(AttributeType::Property, RecordedValueKind::String);
        let shapes = recorded(&[("load", &[shape(AttributeType::Property, RecordedValueKind::Array), text])]);
        let held = recorded(&[("load", &[text])]);
        assert!(ensure_aggregation_eligible(&[attribute("load")], &[AggrMethod::Avg], &shapes, &held).is_err());
        assert!(ensure_aggregation_eligible(&[attribute("load")], &[AggrMethod::Max], &shapes, &held).is_ok());
    }

    /// An attribute the branch never recorded stays eligible for the counting
    /// methods alone, whatever the instances of the request hold.
    #[test]
    fn an_attribute_never_recorded_is_eligible_for_counting_alone() {
        let none = recorded(&[]);
        assert!(ensure_aggregation_eligible(&[attribute("unknown")], &[AggrMethod::Avg], &none, &none).is_err());
        assert!(ensure_aggregation_eligible(&[attribute("unknown")], &COUNTING, &none, &none).is_ok());
    }
}
