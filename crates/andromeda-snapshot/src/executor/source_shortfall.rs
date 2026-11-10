//! The Context Sources a Snapshot query's answer was given without (clauses
//! 5.16.1.4 and 6.3.17).
//!
//! Clause 5.16.1.4 executes a Snapshot query as clause 5.7.2.4 or 5.7.4.4
//! executes it. Andromeda answers a read that a Context Source failed for with
//! what the broker and the other sources hold, and reports the source in an
//! `NGSILD-Warning` (clause 6.3.17), whatever the mode of its registration. The
//! query has not failed, so its execution result is `success` or `empty`
//! (clause 5.2.42). An execution result has no member for warnings; its
//! `problemDetails`, which gives "more details regarding the result status",
//! carries them instead: a problem of the `BadGateway` type Andromeda answers
//! the failure of a single source with, without a `status`, since no error
//! response was given, whose detail lists every warning the answer carried.
//!
//! A warning that cached data stands in for a source (code 110) reports no
//! failure and is left out. A source that answers 404 is not abnormal and
//! carries no warning.

use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_ngsi_ld_problem_details::problem_detail::{ProblemClassification, ProblemDetail};
use indexmap::IndexSet;
use thiserror::Error;

/// The failures of Context Sources one query's answer was given without.
#[derive(Debug, Default)]
pub(super) struct SourceShortfall {
    /// Every warning of a failed source the answer carried, each once, in the
    /// order first met.
    warnings: IndexSet<NgsiLdWarning>,
}

impl SourceShortfall {
    /// Record the warnings one answer of the sources carried. A capture that
    /// asks the sources several times records every answer.
    pub(super) fn record<'w>(&mut self, warnings: impl IntoIterator<Item = &'w NgsiLdWarning>) {
        self.warnings
            .extend(warnings.into_iter().filter(|warning| reports_failure(warning.code())).cloned());
    }

    /// The problem the query's execution result carries, `None` when no
    /// source failed.
    pub(super) fn problem(&self) -> Option<ProblemDetail> {
        (!self.warnings.is_empty()).then(|| {
            ProblemDetail::from_classification(ProblemClassification::BadGateway, UnansweredSources { warnings: &self.warnings }.to_string()).without_status()
        })
    }
}

/// Whether a warning with `code` reports a source that failed (Table
/// 6.3.17-1).
const fn reports_failure(code: WarnCode) -> bool {
    match code {
        WarnCode::ResponseIsStale => false,
        WarnCode::RevalidationFailed | WarnCode::MiscellaneousWarning | WarnCode::MiscellaneousPersistentWarning => true,
    }
}

/// The detail of the problem: the warning the API sends for each source that
/// failed, as its `NGSILD-Warning` value.
#[derive(Debug, Error)]
#[error("the query was answered without the context sources that failed: {}", listed(.warnings))]
struct UnansweredSources<'w> {
    /// The warnings, each once.
    warnings: &'w IndexSet<NgsiLdWarning>,
}

/// The warnings as their `NGSILD-Warning` values, joined by `; `.
fn listed(warnings: &IndexSet<NgsiLdWarning>) -> String {
    warnings.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use crate::executor::source_shortfall::SourceShortfall;
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_ngsi_ld_problem_details::{problem_detail::ProblemClassification, status_member::StatusMember};
    use pretty_assertions::assert_eq;

    fn failed(code: WarnCode, registration: u32) -> NgsiLdWarning {
        NgsiLdWarning::new(code, format!("context source urn:ngsi-ld:ContextSourceRegistration:{registration} failed"))
    }

    #[test]
    fn an_answer_no_source_failed_for_carries_no_problem() {
        let mut shortfall = SourceShortfall::default();
        shortfall.record(&[]);
        assert_eq!(shortfall.problem(), None);
    }

    /// Table 6.3.17-1: code 110 reports cached data standing in for a source,
    /// not a failure.
    #[test]
    fn a_stale_answer_is_not_a_failed_source() {
        let mut shortfall = SourceShortfall::default();
        shortfall.record(&[NgsiLdWarning::new(WarnCode::ResponseIsStale, "cached data")]);
        assert_eq!(shortfall.problem(), None);
    }

    /// No error response was given, so the problem states no status.
    #[test]
    fn a_failed_source_is_a_bad_gateway_problem_without_status() {
        let mut shortfall = SourceShortfall::default();
        shortfall.record(&[failed(WarnCode::MiscellaneousPersistentWarning, 1)]);
        let problem = shortfall.problem().expect("a problem");
        assert_eq!(problem.classification(), ProblemClassification::BadGateway);
        assert_eq!(problem.status_member(), StatusMember::Omitted);
    }

    #[test]
    fn every_failed_source_is_named_once_by_the_warning_the_api_sends_in_the_order_first_met() {
        let mut shortfall = SourceShortfall::default();
        shortfall.record(&[failed(WarnCode::MiscellaneousPersistentWarning, 1), failed(WarnCode::MiscellaneousWarning, 2)]);
        shortfall.record(&[failed(WarnCode::MiscellaneousPersistentWarning, 1), failed(WarnCode::RevalidationFailed, 3)]);
        assert_eq!(
            shortfall.problem().map(|problem| problem.detail().to_owned()),
            Some(
                concat!(
                    "the query was answered without the context sources that failed: ",
                    r#"299 - "context source urn:ngsi-ld:ContextSourceRegistration:1 failed"; "#,
                    r#"199 - "context source urn:ngsi-ld:ContextSourceRegistration:2 failed"; "#,
                    r#"111 - "context source urn:ngsi-ld:ContextSourceRegistration:3 failed""#,
                )
                .to_owned()
            )
        );
    }
}
