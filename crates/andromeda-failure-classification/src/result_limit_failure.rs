//! Classifying a query refused for gathering more than one request may.
//!
//! Clause 4.12 lets a broker refuse a query whose result it will not gather
//! whole, and clause 5.5.2 names the refusal `TooManyResults`, which Table
//! 6.3.2-1 answers with 403. The threshold is the implementation's, so the
//! detail names the setting that holds it.

use crate::classified_report::ClassifiedReport;
use andromeda_federation::{instance_budget::InstanceBudgetExceeded, result_budget::ResultBudgetExceeded};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// Classify a query whose context sources, or whose merged collection, hold
/// more entities or rows than one request gathers.
#[must_use]
pub fn classify_result_budget_refusal(exceeded: &ResultBudgetExceeded) -> ClassifiedReport {
    ClassifiedReport::public(NgsiLdErrorType::TooManyResults, exceeded.to_string())
}

/// Classify a query whose context sources hold more attribute instances than
/// one request gathers.
#[must_use]
pub fn classify_instance_budget_refusal(exceeded: &InstanceBudgetExceeded) -> ClassifiedReport {
    ClassifiedReport::public(NgsiLdErrorType::TooManyResults, exceeded.to_string())
}

#[cfg(test)]
mod tests {
    use crate::result_limit_failure::{classify_instance_budget_refusal, classify_result_budget_refusal};
    use andromeda_federation::{instance_budget::InstanceBudgetExceeded, result_budget::ResultBudgetExceeded};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use http::StatusCode;
    use std::num::NonZeroU32;

    /// Clause 4.12: every limit on what one request gathers is refused as a
    /// 403 `TooManyResults` whose detail names the setting holding the limit.
    #[test]
    fn a_query_past_a_gathering_limit_is_a_403_too_many_results_naming_the_setting() {
        let maximum = NonZeroU32::MIN.saturating_add(4);
        for (report, setting) in [
            (
                classify_result_budget_refusal(&ResultBudgetExceeded::DistinctEntities { maximum }),
                "federation.query.max_entities",
            ),
            (
                classify_result_budget_refusal(&ResultBudgetExceeded::SourceRows { maximum }),
                "federation.query.max_source_rows",
            ),
            (
                classify_instance_budget_refusal(&InstanceBudgetExceeded { maximum }),
                "federation.query.max_instances",
            ),
        ] {
            pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::TooManyResults, "{setting}");
            pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::FORBIDDEN, "{setting}");
            assert!(report.detail().contains(setting), "{}", report.detail());
            assert!(report.detail().contains('5'), "{}", report.detail());
        }
    }
}
