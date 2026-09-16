//! What one aggregation group spans, and whether that span is answerable.
//!
//! Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
//! period, so [`crate::aggregation`] groups on exactly those three. Everything
//! else the rows of a group may disagree about — the `unitCode` of clause
//! 4.5.2, the attribute family of clause 5.2.4, the shape the value was
//! recorded in — is projected as the span of the group instead of keying it.
//!
//! A span that is not a single value is not automatically a refusal. `avg` over
//! a kilogram and a pound is a number nobody can interpret, and `min` over a
//! string and a number has two equally valid answers, so those are refused
//! rather than computed; `totalCount` and `distinctCount` count instances, and
//! how many instances there were is the same question whatever the instances
//! carried, so counting is answered over any span at all.

use crate::recorded_shape::RecordedShape;
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName};
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use thiserror::Error;

/// The lowest and highest code one aggregation group's rows carried in a
/// registry column.
///
/// Both bounds are kept rather than a "uniform or not" flag, because the error
/// that refuses a mixed group names the two codes that contradicted each other.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CodeSpan<Code> {
    /// The lowest code observed, and the one a surviving mixed group is reported under.
    pub lowest: Code,
    /// The highest code observed, equal to [`lowest`](CodeSpan::lowest) when the group is uniform.
    pub highest: Code,
}

impl<Code: Copy + Eq> CodeSpan<Code> {
    /// The one code the whole group carried, absent when its rows disagreed.
    pub fn uniform(self) -> Option<Code> {
        (self.lowest == self.highest).then_some(self.lowest)
    }
}

/// The unit identities one aggregation group's rows carried (clause 4.5.2 `unitCode`).
///
/// A unit and its absence are distinct: an unannotated value is not a value in
/// the annotated unit, so a group holding both is as undefined as one holding
/// two different units.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObservedUnits {
    /// Every row carried the same unit code, or every row carried none.
    Uniform(Option<UnitCodeId>),
    /// Two rows carried different unit codes.
    Conflicting {
        /// The lower of the two contradicting codes.
        first: UnitCodeId,
        /// The higher of the two contradicting codes.
        second: UnitCodeId,
    },
    /// Some rows carried a unit code and others carried none at all.
    UnitAndUnitless {
        /// The unit the annotated rows carried.
        present: UnitCodeId,
    },
}

/// Everything one aggregation group's rows disagreed about, or agreed on.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AggregationGroupSpan {
    /// Expanded name of the aggregated attribute.
    pub attribute_name: AttributeName,
    /// The unit identities the group's rows carried.
    pub units: ObservedUnits,
    /// The attribute families the group's rows were written as (clause 5.2.4).
    pub families: CodeSpan<AttributeKind>,
    /// The value shapes the group's rows were recorded in, at the grain Tables
    /// 4.5.19.1-1 and 4.5.19.1-2 distinguish rather than the grain the row's
    /// typed value column does.
    pub value_shapes: CodeSpan<RecordedShape>,
}

/// Whether an aggregate's result depends on more about its inputs than how many there were.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MixingSensitivity {
    /// The result is derived from the values themselves, so their unit and shape decide what it means.
    Sensitive,
    /// The result counts instances, so what each one carried is irrelevant.
    Insensitive,
}

/// Classify an aggregation method of clause 4.5.19 by what its result depends on.
#[must_use]
pub const fn mixing_sensitivity(method: AggrMethod) -> MixingSensitivity {
    match method {
        AggrMethod::Avg | AggrMethod::Min | AggrMethod::Max | AggrMethod::Sum | AggrMethod::SumSq | AggrMethod::StdDev => MixingSensitivity::Sensitive,
        AggrMethod::TotalCount | AggrMethod::DistinctCount => MixingSensitivity::Insensitive,
    }
}

/// A value-sensitive aggregate would have had to span inputs that do not share a meaning.
///
/// Recommendation 20 carries unit conversion factors only informatively and
/// NGSI-LD asks no broker to apply them, so an aggregate over values that do not
/// share a scale is refused rather than computed. The same holds for a period
/// whose rows changed attribute family or value shape: clause 4.5.19 defines one
/// aggregate value per period, so the answer cannot be split in two, and merging
/// families would report an extremum of values that were never comparable.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MixedAggregationGroup {
    /// Two different unit codes were recorded inside one aggregation group.
    #[error("attribute '{attribute_name}' aggregates over unit codes {first} and {second}, which the broker does not convert between")]
    ConflictingUnits {
        /// Expanded name of the aggregated attribute.
        attribute_name: AttributeName,
        /// The lower of the two contradicting codes.
        first: UnitCodeId,
        /// The higher of the two contradicting codes.
        second: UnitCodeId,
    },
    /// Values with a unit and values without one were recorded inside one aggregation group.
    #[error("attribute '{attribute_name}' aggregates over values carrying unit code {present} and values carrying no unit at all")]
    UnitAndUnitless {
        /// Expanded name of the aggregated attribute.
        attribute_name: AttributeName,
        /// The unit the annotated values carried.
        present: UnitCodeId,
    },
    /// One aggregation group's rows were written as two different attribute families.
    #[error("attribute '{attribute_name}' aggregates over instances written as {} and as {}, which are different attribute families", .first.label(), .second.label())]
    ConflictingFamilies {
        /// Expanded name of the aggregated attribute.
        attribute_name: AttributeName,
        /// The lower of the two contradicting families.
        first: AttributeKind,
        /// The higher of the two contradicting families.
        second: AttributeKind,
    },
    /// One aggregation group's rows recorded their values in two different shapes.
    #[error("attribute '{attribute_name}' aggregates over {first} and {second} values, which have no common ordering or scale")]
    ConflictingValueShapes {
        /// Expanded name of the aggregated attribute.
        attribute_name: AttributeName,
        /// The lower of the two contradicting shapes.
        first: RecordedShape,
        /// The higher of the two contradicting shapes.
        second: RecordedShape,
    },
}

/// The facets an `aggrGroupBy` promoted from a spanned property to a group key.
///
/// A facet used as a grouping key splits the groups rather than contradicting
/// them: `aggrGroupBy=unitCode` makes a formerly-refused mixed-unit request
/// succeed as one group per unit, and grouping by the attribute family
/// (`type`) does the same for a family that changed within a period. A promoted
/// facet is therefore skipped by [`ensure_uniform_aggregation_groups`], because
/// the SQL `GROUP BY` has already made it uniform within each group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PromotedGroupFacets {
    /// Whether `unitCode` is a grouping key.
    pub units: bool,
    /// Whether the attribute family (`type`) is a grouping key.
    pub families: bool,
}

/// Refuse an aggregated result set whose value-sensitive aggregates span inputs that disagree.
///
/// `groups` is the statement's output, one entry per attribute, dataset, period
/// and grouping key. Methods that only count are unaffected, so a request made
/// up entirely of `totalCount` and `distinctCount` — including the empty method
/// list, which counts by default — is answered over any span unchanged. A facet
/// named in `promoted` is a grouping key rather than a spanned property, so its
/// uniformity is not a precondition but a consequence of the grouping, and its
/// check is skipped.
///
/// # Errors
///
/// Returns the first contradiction found, naming the attribute it was found
/// under and the two values that contradicted each other.
pub fn ensure_uniform_aggregation_groups<'a, Groups>(groups: Groups, methods: &[AggrMethod], promoted: PromotedGroupFacets) -> Result<(), MixedAggregationGroup>
where
    Groups: IntoIterator<Item = &'a AggregationGroupSpan>,
{
    if !methods
        .iter()
        .copied()
        .map(mixing_sensitivity)
        .any(|sensitivity| sensitivity == MixingSensitivity::Sensitive)
    {
        return Ok(());
    }

    for group in groups {
        // The error names the attribute, so it owns a copy of a name the caller
        // keeps: the borrow ends with the result set.
        let attribute_name = || group.attribute_name.clone();
        if !promoted.units {
            match group.units {
                ObservedUnits::Uniform(_) => {}
                ObservedUnits::Conflicting { first, second } => {
                    return Err(MixedAggregationGroup::ConflictingUnits {
                        attribute_name: attribute_name(),
                        first,
                        second,
                    });
                }
                ObservedUnits::UnitAndUnitless { present } => {
                    return Err(MixedAggregationGroup::UnitAndUnitless {
                        attribute_name: attribute_name(),
                        present,
                    });
                }
            }
        }
        if !promoted.families && group.families.uniform().is_none() {
            return Err(MixedAggregationGroup::ConflictingFamilies {
                attribute_name: attribute_name(),
                first: group.families.lowest,
                second: group.families.highest,
            });
        }
        if group.value_shapes.uniform().is_none() {
            return Err(MixedAggregationGroup::ConflictingValueShapes {
                attribute_name: attribute_name(),
                first: group.value_shapes.lowest,
                second: group.value_shapes.highest,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        aggregation_group::{
            AggregationGroupSpan,
            CodeSpan,
            MixedAggregationGroup,
            MixingSensitivity,
            ObservedUnits,
            PromotedGroupFacets,
            ensure_uniform_aggregation_groups,
            mixing_sensitivity,
        },
        recorded_shape::RecordedShape,
    };
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName};
    use andromeda_postgresql_codes::attribute_kind::AttributeKind;
    use andromeda_unit_catalog::unit_code_id::UnitCodeId;

    const WEIGHT: &str = "https://uri.etsi.org/ngsi-ld/default-context/weight";

    fn name(raw: &str) -> AttributeName {
        raw.parse::<AttributeName>().expect("valid attribute name")
    }

    fn unit(id: i16) -> UnitCodeId {
        UnitCodeId::try_from(id).expect("a registered unit identity")
    }

    /// A group whose rows agreed about everything, for tests that vary one facet.
    fn uniform_group(attribute: &str, units: ObservedUnits) -> AggregationGroupSpan {
        AggregationGroupSpan {
            attribute_name: name(attribute),
            units,
            families: CodeSpan {
                lowest: AttributeKind::Property,
                highest: AttributeKind::Property,
            },
            value_shapes: CodeSpan {
                lowest: RecordedShape::Number,
                highest: RecordedShape::Number,
            },
        }
    }

    #[test]
    fn a_group_recorded_in_one_unit_is_accepted_unchanged() {
        let groups = [uniform_group(WEIGHT, ObservedUnits::Uniform(Some(unit(367))))];
        pretty_assertions::assert_eq!(
            ensure_uniform_aggregation_groups(&groups, &[AggrMethod::Avg], PromotedGroupFacets::default()),
            Ok(())
        );
    }

    #[test]
    fn an_attribute_that_carries_no_unit_aggregates_normally() {
        let groups = [uniform_group(
            "https://uri.etsi.org/ngsi-ld/default-context/occupancy",
            ObservedUnits::Uniform(None),
        )];
        pretty_assertions::assert_eq!(
            ensure_uniform_aggregation_groups(&groups, &[AggrMethod::Avg, AggrMethod::StdDev], PromotedGroupFacets::default()),
            Ok(())
        );
    }

    #[test]
    fn a_group_spanning_two_units_is_refused() {
        let groups = [uniform_group(
            WEIGHT,
            ObservedUnits::Conflicting {
                first: unit(1121),
                second: unit(1407),
            },
        )];
        let expected = MixedAggregationGroup::ConflictingUnits {
            attribute_name: name(WEIGHT),
            first: unit(1121),
            second: unit(1407),
        };
        pretty_assertions::assert_eq!(
            ensure_uniform_aggregation_groups(&groups, &[AggrMethod::Avg], PromotedGroupFacets::default()),
            Err(expected)
        );
    }

    #[test]
    fn a_group_mixing_annotated_and_unannotated_values_is_refused() {
        let groups = [uniform_group(WEIGHT, ObservedUnits::UnitAndUnitless { present: unit(1121) })];
        let expected = MixedAggregationGroup::UnitAndUnitless {
            attribute_name: name(WEIGHT),
            present: unit(1121),
        };
        pretty_assertions::assert_eq!(
            ensure_uniform_aggregation_groups(&groups, &[AggrMethod::Sum], PromotedGroupFacets::default()),
            Err(expected)
        );
    }

    /// A period whose rows changed attribute family cannot be answered with one
    /// aggregate under one family, and clause 4.5.19 allows no second entry for
    /// the same period to put the other one in.
    #[test]
    fn a_group_spanning_two_attribute_families_is_refused() {
        let mut group = uniform_group(WEIGHT, ObservedUnits::Uniform(None));
        group.families = CodeSpan {
            lowest: AttributeKind::Property,
            highest: AttributeKind::Relationship,
        };
        let expected = MixedAggregationGroup::ConflictingFamilies {
            attribute_name: name(WEIGHT),
            first: AttributeKind::Property,
            second: AttributeKind::Relationship,
        };
        pretty_assertions::assert_eq!(
            ensure_uniform_aggregation_groups(&[group], &[AggrMethod::Min], PromotedGroupFacets::default()),
            Err(expected)
        );
    }

    /// `min` reads the typed column the value shape names, so a group holding
    /// both a number and a string has two equally valid extrema and neither is
    /// the answer.
    #[test]
    fn a_group_spanning_two_value_shapes_is_refused() {
        let mut group = uniform_group(WEIGHT, ObservedUnits::Uniform(None));
        group.value_shapes = CodeSpan {
            lowest: RecordedShape::Number,
            highest: RecordedShape::String,
        };
        let expected = MixedAggregationGroup::ConflictingValueShapes {
            attribute_name: name(WEIGHT),
            first: RecordedShape::Number,
            second: RecordedShape::String,
        };
        pretty_assertions::assert_eq!(
            ensure_uniform_aggregation_groups(&[group], &[AggrMethod::Max], PromotedGroupFacets::default()),
            Err(expected)
        );
    }

    /// Counting instances says nothing about the scale they were measured on or
    /// the shape they were recorded in, so every span stays answerable — and an
    /// empty method list counts by default.
    #[test]
    fn counting_aggregates_survive_every_kind_of_mixing() {
        let mut group = uniform_group(
            WEIGHT,
            ObservedUnits::Conflicting {
                first: unit(1121),
                second: unit(1407),
            },
        );
        group.families = CodeSpan {
            lowest: AttributeKind::Property,
            highest: AttributeKind::Relationship,
        };
        group.value_shapes = CodeSpan {
            lowest: RecordedShape::Number,
            highest: RecordedShape::Uri,
        };
        let groups = [group];
        for methods in [
            vec![AggrMethod::TotalCount],
            vec![AggrMethod::TotalCount, AggrMethod::DistinctCount],
            Vec::new(),
        ] {
            pretty_assertions::assert_eq!(
                ensure_uniform_aggregation_groups(&groups, &methods, PromotedGroupFacets::default()),
                Ok(()),
                "{methods:?}"
            );
        }
    }

    /// One sensitive method among counting ones is enough to make the request
    /// depend on the values, so the refusal is not diluted by the counts.
    #[test]
    fn one_sensitive_method_beside_counting_ones_still_refuses_a_mixed_group() {
        let groups = [uniform_group(
            WEIGHT,
            ObservedUnits::Conflicting {
                first: unit(1121),
                second: unit(1407),
            },
        )];
        assert!(ensure_uniform_aggregation_groups(&groups, &[AggrMethod::TotalCount, AggrMethod::Avg], PromotedGroupFacets::default()).is_err());
    }

    #[test]
    fn two_attributes_may_each_hold_their_own_unit() {
        let groups = [
            uniform_group(WEIGHT, ObservedUnits::Uniform(Some(unit(1121)))),
            uniform_group("https://uri.etsi.org/ngsi-ld/default-context/length", ObservedUnits::Uniform(Some(unit(1407)))),
        ];
        pretty_assertions::assert_eq!(
            ensure_uniform_aggregation_groups(&groups, &[AggrMethod::Max], PromotedGroupFacets::default()),
            Ok(())
        );
    }

    #[test]
    fn every_aggregation_method_is_classified_by_what_its_result_depends_on() {
        for method in [
            AggrMethod::Avg,
            AggrMethod::Min,
            AggrMethod::Max,
            AggrMethod::Sum,
            AggrMethod::SumSq,
            AggrMethod::StdDev,
        ] {
            pretty_assertions::assert_eq!(mixing_sensitivity(method), MixingSensitivity::Sensitive, "{method}");
        }
        for method in [AggrMethod::TotalCount, AggrMethod::DistinctCount] {
            pretty_assertions::assert_eq!(mixing_sensitivity(method), MixingSensitivity::Insensitive, "{method}");
        }
    }

    /// `aggrGroupBy=unitCode` promotes the unit to a key, so a span that would
    /// be refused as `ConflictingUnits` is accepted: the SQL splits it into one
    /// group per unit and the guard no longer checks the facet.
    #[test]
    fn promoting_the_unit_to_a_key_accepts_a_formerly_conflicting_unit_span() {
        let groups = [uniform_group(
            WEIGHT,
            ObservedUnits::Conflicting {
                first: unit(1121),
                second: unit(1407),
            },
        )];
        let promoted = PromotedGroupFacets { units: true, families: false };
        pretty_assertions::assert_eq!(ensure_uniform_aggregation_groups(&groups, &[AggrMethod::Avg], promoted), Ok(()));
        // Without the promotion the same span is still refused.
        assert!(ensure_uniform_aggregation_groups(&groups, &[AggrMethod::Avg], PromotedGroupFacets::default()).is_err());
    }

    /// Promoting the attribute family (`type`) likewise accepts a span that
    /// changed family, while the unit and shape checks stay in force.
    #[test]
    fn promoting_the_family_to_a_key_accepts_a_formerly_conflicting_family_span() {
        let mut group = uniform_group(WEIGHT, ObservedUnits::Uniform(None));
        group.families = CodeSpan {
            lowest: AttributeKind::Property,
            highest: AttributeKind::Relationship,
        };
        let promoted = PromotedGroupFacets { units: false, families: true };
        pretty_assertions::assert_eq!(ensure_uniform_aggregation_groups(&[group.clone()], &[AggrMethod::Min], promoted), Ok(()));
        assert!(ensure_uniform_aggregation_groups(&[group], &[AggrMethod::Min], PromotedGroupFacets::default()).is_err());
    }

    #[test]
    fn a_span_reports_the_one_code_it_covers_and_nothing_when_it_covers_two() {
        let uniform = CodeSpan {
            lowest: RecordedShape::Number,
            highest: RecordedShape::Number,
        };
        pretty_assertions::assert_eq!(uniform.uniform(), Some(RecordedShape::Number));
        let mixed = CodeSpan {
            lowest: RecordedShape::Number,
            highest: RecordedShape::String,
        };
        pretty_assertions::assert_eq!(mixed.uniform(), None);
    }
}
