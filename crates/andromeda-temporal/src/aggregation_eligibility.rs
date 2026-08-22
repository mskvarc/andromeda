//! Whether the attributes of an aggregated temporal representation can be
//! aggregated by the methods requested (clause 5.7.4.4, Tables 4.5.19.1-1 and
//! 4.5.19.1-2).

use crate::port::history_reader::RecordedValueKind;
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName};
use andromeda_ngsi_ld_request::error::ParameterError;
use std::collections::{HashMap, HashSet};

/// Check every requested attribute against every requested method.
///
/// An attribute is eligible for a method when every value shape it has been
/// recorded with admits the method. An attribute never recorded has no values
/// to aggregate and is eligible only for the counting methods, which need no
/// value shape at all.
///
/// # Errors
///
/// [`ParameterError::AggregationIneligible`] naming the first attribute and
/// method the tables do not admit.
pub fn ensure_aggregation_eligible(
    attributes: &[AttributeName],
    methods: &[AggrMethod],
    recorded: &HashMap<AttributeName, HashSet<RecordedValueKind>>,
) -> Result<(), ParameterError> {
    for attribute in attributes {
        for method in methods {
            let eligible = match recorded.get(attribute) {
                Some(kinds) if !kinds.is_empty() => kinds.iter().all(|kind| admits(*kind, *method)),
                Some(_) | None => matches!(method, AggrMethod::TotalCount | AggrMethod::DistinctCount),
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

/// Tables 4.5.19.1-1 and 4.5.19.1-2: whether `method` is defined over values
/// of shape `kind`. A stored object stands for both JSON objects and JSON
/// arrays, and the table admits an array only where it admits an object too.
const fn admits(kind: RecordedValueKind, method: AggrMethod) -> bool {
    match method {
        AggrMethod::TotalCount | AggrMethod::DistinctCount => true,
        AggrMethod::Avg | AggrMethod::Sum => matches!(kind, RecordedValueKind::Number | RecordedValueKind::Boolean),
        AggrMethod::SumSq | AggrMethod::StdDev => matches!(kind, RecordedValueKind::Number | RecordedValueKind::Boolean),
        AggrMethod::Min | AggrMethod::Max => matches!(
            kind,
            RecordedValueKind::Number | RecordedValueKind::Boolean | RecordedValueKind::String | RecordedValueKind::DateTime
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::{aggregation_eligibility::ensure_aggregation_eligible, port::history_reader::RecordedValueKind};
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName};
    use andromeda_ngsi_ld_request::error::ParameterError;
    use std::collections::{HashMap, HashSet};

    fn attribute(name: &str) -> AttributeName {
        name.parse().expect("valid attribute name")
    }

    fn recorded(entries: &[(&str, &[RecordedValueKind])]) -> HashMap<AttributeName, HashSet<RecordedValueKind>> {
        entries.iter().map(|(name, kinds)| (attribute(name), kinds.iter().copied().collect())).collect()
    }

    /// Table 4.5.19.1-1: `sum` is not defined over strings, so an attribute
    /// recorded with string values is ineligible for it, while a numeric one
    /// is eligible for every method.
    #[test]
    fn a_string_attribute_is_ineligible_for_a_numeric_method() {
        let kinds = recorded(&[("brandName", &[RecordedValueKind::String]), ("speed", &[RecordedValueKind::Number])]);
        let error = ensure_aggregation_eligible(&[attribute("brandName")], &[AggrMethod::Sum], &kinds).expect_err("strings cannot be summed");
        assert!(
            matches!(error, ParameterError::AggregationIneligible { method: AggrMethod::Sum, .. }),
            "{error:?}"
        );
        for method in [
            AggrMethod::Avg,
            AggrMethod::Min,
            AggrMethod::Max,
            AggrMethod::Sum,
            AggrMethod::SumSq,
            AggrMethod::StdDev,
            AggrMethod::TotalCount,
            AggrMethod::DistinctCount,
        ] {
            assert!(ensure_aggregation_eligible(&[attribute("speed")], &[method], &kinds).is_ok(), "{method}");
        }
    }

    /// Strings admit `min`/`max` and the counting methods; objects admit only
    /// the counting methods.
    #[test]
    fn strings_admit_ordering_and_objects_admit_only_counting() {
        let kinds = recorded(&[("name", &[RecordedValueKind::String]), ("config", &[RecordedValueKind::Object])]);
        assert!(ensure_aggregation_eligible(&[attribute("name")], &[AggrMethod::Min, AggrMethod::Max, AggrMethod::TotalCount], &kinds).is_ok());
        assert!(ensure_aggregation_eligible(&[attribute("name")], &[AggrMethod::Avg], &kinds).is_err());
        assert!(ensure_aggregation_eligible(&[attribute("config")], &[AggrMethod::DistinctCount, AggrMethod::TotalCount], &kinds).is_ok());
        assert!(ensure_aggregation_eligible(&[attribute("config")], &[AggrMethod::Max], &kinds).is_err());
    }

    /// An attribute recorded with several shapes is eligible only for the
    /// methods every shape admits; one never recorded only for counting.
    #[test]
    fn mixed_and_unrecorded_attributes_are_held_to_the_strictest_shape() {
        let kinds = recorded(&[("mixed", &[RecordedValueKind::Number, RecordedValueKind::String])]);
        assert!(ensure_aggregation_eligible(&[attribute("mixed")], &[AggrMethod::Max], &kinds).is_ok());
        assert!(ensure_aggregation_eligible(&[attribute("mixed")], &[AggrMethod::Sum], &kinds).is_err());
        assert!(ensure_aggregation_eligible(&[attribute("unknown")], &[AggrMethod::TotalCount], &kinds).is_ok());
        assert!(ensure_aggregation_eligible(&[attribute("unknown")], &[AggrMethod::Avg], &kinds).is_err());
    }
}
