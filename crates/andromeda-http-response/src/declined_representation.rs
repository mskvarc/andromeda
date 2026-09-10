//! The answer a distributed read gives when the one registered Context Source
//! holding its data declined the requested representation.
//!
//! Clause 6.3.7 lets a Context Source answer 501 Not Implemented when it does
//! not support the data format the request asks for, and the response tables
//! of the operations a broker forwards -- Table 6.4.3.2-3 for the entity reads,
//! Table 6.34.3.1-1 for the creation of an `EntityMap` -- list that same 501
//! as an answer the broker itself gives, "used by Registered Context Sources
//! to indicate that the data format of the request is unsupported". It stands
//! for the operation only where that one source holds all of the data, which
//! is what the fan-out decides; the rest of the decision is what this module
//! renders.

use crate::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
use axum::http::Uri;

/// The response a read gives when the one exclusive or redirect source holding
/// all of its data declined the representation with `status`.
///
/// `instance` is the request URI the resulting `ProblemDetails` names.
#[must_use]
pub fn single_source_declined(status: u16, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public_binding(
        ProblemClassification::NotImplemented,
        format!("the registered context source holding the requested data answered HTTP {status}: it does not support the requested representation"),
    )
    .with_instance(instance.clone()) // clone: the problem detail owns the instance it names
}

#[cfg(test)]
mod tests {
    use crate::declined_representation::single_source_declined;
    use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
    use axum::http::Uri;

    /// Table 6.4.3.2-3 and Table 6.34.3.1-1 both give the relay the source's
    /// own 501, and clause 6.3.7 makes it a `ProblemDetails` body.
    #[test]
    fn a_declined_representation_is_answered_with_the_not_implemented_status() {
        let instance: Uri = "/ngsi-ld/v1/entities".parse().expect("valid request URI");
        let response = single_source_declined(501, &instance);
        pretty_assertions::assert_eq!(response.problem().classification(), ProblemClassification::NotImplemented);
        pretty_assertions::assert_eq!(response.problem().status_code().as_u16(), 501);
    }

    /// The relayed status travels into the detail, so a client sees which
    /// answer the broker is passing on rather than only the category.
    #[test]
    fn the_relayed_status_is_named_in_the_client_visible_detail() {
        let instance: Uri = "/ngsi-ld/v1/entityMaps".parse().expect("valid request URI");
        let response = single_source_declined(501, &instance);
        let detail = response.problem().detail();
        assert!(detail.contains("501"), "{detail}");
    }
}
