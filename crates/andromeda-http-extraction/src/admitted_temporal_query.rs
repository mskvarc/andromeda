//! The temporal query a request carries, extracted already past the broker's
//! own admission of it.
//!
//! The aggregation-bearing temporal query bindings -- `GET /temporal/entities`,
//! `GET /temporal/entities/{id}`, `POST /temporal/entityOperations/query` and
//! the temporal Entity Map creations over GET and POST -- read their parameters
//! and then refuse a query the broker does not enable (an extension that is
//! switched off must behave as one never implemented).
//! Extracting the parameters through these types makes that refusal part of
//! reading them, so no binding can read the parameters and forget it. The
//! admission itself is the route's: it is installed as a request extension of
//! the [`TemporalQueryAdmission`] type the binding names.

use crate::{addressed_uri::addressed_uri, jsonld::JsonLd, query::ValidatedQuery, submitted_body::SubmittedBody};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    extraction_rejection::ExtractorRejection,
    internal_failure_summary::InternalFailureSummary,
    parameter_failure::classify_parameter_error,
};
use andromeda_jsonld::user_context_admission::SubmittedContext;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_request::{
    empty::EmptyParams,
    query::temporal::{TemporalQueryParameters, post_parameters::TemporalPostQueryParams},
    query_parameter::QueryParameterSet,
};
use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    http::request::Parts,
};
use serde::de::DeserializeOwned;
use std::marker::PhantomData;
use thiserror::Error;

/// Where a temporal query's parameters were written.
///
/// An extension parameter is undefined on the URL bindings when its extension
/// is off, while a member of the clause 5.2.23 `Query` body is defined whatever
/// the broker enables, so the two are refused differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalQueryBinding {
    /// The URL query string of a `GET` (clauses 6.18.3.2 and 6.19.3.1).
    QueryString,
    /// The clause 5.2.23 body of a `POST` (clause 6.24.3.1).
    Body,
}

/// A route's admission of the temporal queries it answers.
pub trait TemporalQueryAdmission: Clone + Send + Sync + 'static {
    /// Admit `params`, written on `binding`, or refuse them.
    ///
    /// # Errors
    ///
    /// Returns the report the refused query is answered with.
    fn admit(&self, params: &TemporalQueryParameters, binding: TemporalQueryBinding) -> Result<(), ClassifiedReport>;
}

/// The route installed no admission of the type its binding names: a wiring
/// fault of the broker, never a client's.
#[derive(Debug, Error)]
#[error("the temporal query route installs no admission of the type its binding reads")]
struct AdmissionNotInstalled;

/// The URL parameters of a GET temporal binding, decoded as the operation's
/// own parameter set `P` and admitted by the route's `A`.
///
/// `P` is what the binding's table defines: the whole temporal query for
/// Query Temporal Evolution of Entities and the Entity Map it creates
/// (clauses 6.18.3.2 and 6.35.3.1), the retrieval's narrower set for Retrieve
/// Temporal Evolution of an Entity (clause 6.19.3.1).
pub struct AdmittedTemporalQuery<A, P = TemporalQueryParameters> {
    /// The admitted parameters.
    pub params: TemporalQueryParameters,
    /// The admission the parameters passed and the set they were decoded as.
    admission: PhantomData<fn() -> (A, P)>,
}

impl<S, A, P> FromRequestParts<S> for AdmittedTemporalQuery<A, P>
where
    S: Send + Sync,
    A: TemporalQueryAdmission,
    P: DeserializeOwned + QueryParameterSet + Send + Into<TemporalQueryParameters>,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<AdmittedTemporalQuery<A, P>, ExtractorRejection> {
        let ValidatedQuery(decoded) = ValidatedQuery::<P>::from_request_parts(parts, state).await?;
        let params = decoded.into();
        let uri = addressed_uri(parts);
        installed_admission::<A>(parts)?
            .admit(&params, TemporalQueryBinding::QueryString)
            .map_err(|report| ExtractorRejection::Classified(report.with_instance(uri)))?;
        Ok(AdmittedTemporalQuery {
            params,
            admission: PhantomData,
        })
    }
}

/// The URL parameters a POST temporal binding carries beside its clause 5.2.23
/// body.
pub trait BodyBindingParameters: DeserializeOwned + QueryParameterSet + Send {
    /// Set the URL parameters on the query the body produced.
    fn apply_to(self, params: &mut TemporalQueryParameters);
}

// Clauses 6.3.10, 6.3.11, 6.3.12, 6.3.13 and 6.3.18 grant POST Query Temporal
// Evolution of Entities the URL parameters its body has no member for.
impl BodyBindingParameters for TemporalPostQueryParams {
    fn apply_to(self, params: &mut TemporalQueryParameters) {
        TemporalPostQueryParams::apply_to(self, params);
    }
}

// Table 6.35.3.2-1 gives the temporal Entity Map creation via POST no URL
// parameter: the body is the whole query.
impl BodyBindingParameters for EmptyParams {
    fn apply_to(self, _params: &mut TemporalQueryParameters) {}
}

/// The clause 5.2.23 body and URL parameters `U` of a POST temporal binding
/// (clauses 6.24.3.1 and 6.35.3.2), folded into one parameter set admitted by
/// the route's `A`, with the one context the body extractor resolved (clause
/// 6.3.5).
pub struct AdmittedTemporalPostQuery<A, U = TemporalPostQueryParams> {
    /// The query the body and the URL parameters describe, read once the
    /// context is admitted.
    pub params: SubmittedBody<TemporalQueryParameters>,
    /// The context the query is read against: the body's own `@context` for
    /// `application/ld+json`, the `Link` context for `application/json`.
    pub context: SubmittedContext,
    /// The admission the parameters passed and the URL parameters the binding
    /// defines.
    admission: PhantomData<fn() -> (A, U)>,
}

impl<S, A, U> FromRequest<S> for AdmittedTemporalPostQuery<A, U>
where
    S: Send + Sync,
    A: TemporalQueryAdmission,
    U: BodyBindingParameters + 'static,
{
    type Rejection = ExtractorRejection;

    async fn from_request(request: Request, state: &S) -> Result<AdmittedTemporalPostQuery<A, U>, ExtractorRejection> {
        let (mut parts, body) = request.into_parts();
        let ValidatedQuery(url_params) = ValidatedQuery::<U>::from_request_parts(&mut parts, state).await?;
        let admission = installed_admission::<A>(&parts)?;
        let JsonLd { body, context } = JsonLd::<Query>::from_request(Request::from_parts(parts, body), state).await?;
        let params = body.and_then(move |query, uri| {
            // clone: each refusal names the request, which the held body keeps naming
            let mut params = TemporalQueryParameters::try_from(query).map_err(|error| classify_parameter_error(&error).with_instance(uri.clone()))?;
            url_params.apply_to(&mut params);
            // The body's `aggrParams.aggrGroupBy` is folded into `params` by
            // now, so the admission sees the whole query.
            admission
                .admit(&params, TemporalQueryBinding::Body)
                .map_err(|report| report.with_instance(uri.clone()))?; // clone: as above
            Ok(params)
        });
        Ok(AdmittedTemporalPostQuery {
            params,
            context,
            admission: PhantomData,
        })
    }
}

/// The admission `A` the route installed.
///
/// # Errors
///
/// Returns an internal error when the route installed none, which is a wiring
/// fault of the broker.
fn installed_admission<A: TemporalQueryAdmission>(parts: &Parts) -> Result<A, ExtractorRejection> {
    parts
        .extensions
        .get::<A>()
        .cloned() // clone: the admission outlives the parts the body extractor consumes
        .ok_or_else(|| {
            let report = ClassifiedReport::internal(&AdmissionNotInstalled, InternalFailureSummary::new("failed to admit the temporal query"));
            ExtractorRejection::Classified(report.with_instance(addressed_uri(parts)))
        })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::admitted_temporal_query::{AdmittedTemporalPostQuery, AdmittedTemporalQuery, TemporalQueryAdmission, TemporalQueryBinding};
    use andromeda_http_response::{classified_report::ClassifiedReport, extraction_rejection::ExtractorRejection};
    use andromeda_jsonld::user_context_admission::AdmittedContext;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemDetail};
    use andromeda_ngsi_ld_request::{
        empty::EmptyParams,
        query::temporal::{TemporalQueryParameters, retrieve_parameters::TemporalRetrieveParameters},
        query_parameter::QueryParameterSet,
    };
    use axum::{
        body::Body,
        extract::{FromRequest, FromRequestParts},
        http::{
            Request,
            header::{CONTENT_LENGTH, CONTENT_TYPE},
        },
    };
    use serde::de::DeserializeOwned;

    /// An admission standing in for a route's extension gate: it refuses a
    /// grouped query, and any query carrying `q`.
    #[derive(Clone)]
    struct StandInGate;

    impl TemporalQueryAdmission for StandInGate {
        fn admit(&self, params: &TemporalQueryParameters, _binding: TemporalQueryBinding) -> Result<(), ClassifiedReport> {
            match (&params.aggr_group_by, &params.q) {
                (Some(_), _) | (None, Some(_)) => Err(ClassifiedReport::public(
                    NgsiLdErrorType::InvalidRequest,
                    "refused by the admission".to_string(),
                )),
                (None, None) => Ok(()),
            }
        }
    }

    /// The error type a GET binding decoding `P` refuses `uri` with, or `None`
    /// when it admits it.
    async fn refusal_of_get<P>(uri: &str) -> Option<NgsiLdErrorType>
    where
        P: DeserializeOwned + QueryParameterSet + Send + Into<TemporalQueryParameters>,
    {
        let request = Request::builder().uri(uri).extension(StandInGate).body(()).expect("a valid request");
        let (mut parts, ()) = request.into_parts();
        AdmittedTemporalQuery::<StandInGate, P>::from_request_parts(&mut parts, &())
            .await
            .err()
            .and_then(|rejection: ExtractorRejection| rejection.problem().map(ProblemDetail::error_type))
    }

    /// Table 6.19.3.1-1: the retrieval selects no entity, so `type` is refused
    /// while the query reads it.
    #[tokio::test]
    async fn the_retrieval_set_refuses_what_only_the_query_defines() {
        let uri = "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:V:1?type=Vehicle";
        pretty_assertions::assert_eq!(refusal_of_get::<TemporalRetrieveParameters>(uri).await, Some(NgsiLdErrorType::InvalidRequest));
        pretty_assertions::assert_eq!(refusal_of_get::<TemporalQueryParameters>(uri).await, None);
    }

    /// The route's admission still runs over the narrower set.
    #[tokio::test]
    async fn the_admission_runs_over_the_retrieval_set() {
        let grouped = "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:V:1?format=groupedValues&aggrMethods=sum&aggrGroupBy=unitCode";
        pretty_assertions::assert_eq!(
            refusal_of_get::<TemporalRetrieveParameters>(grouped).await,
            Some(NgsiLdErrorType::InvalidRequest)
        );
        let plain = "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:V:1?lastN=1&pick=speed";
        pretty_assertions::assert_eq!(refusal_of_get::<TemporalRetrieveParameters>(plain).await, None);
    }

    /// A POST temporal binding whose URL defines nothing, reading `body`.
    fn post(uri: &str, body: &'static str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .extension(StandInGate)
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .expect("a valid request")
    }

    const WINDOWED: &str = r#"{"type": "Query", "entities": [{"type": "Vehicle"}], "temporalQ": {"timerel": "before", "timeAt": "2030-01-01T00:00:00Z"}}"#;
    const FILTERED: &str =
        r#"{"type": "Query", "entities": [{"type": "Vehicle"}], "q": "speed>1", "temporalQ": {"timerel": "before", "timeAt": "2030-01-01T00:00:00Z"}}"#;

    /// Table 6.35.3.2-1: the temporal Entity Map creation via POST defines no
    /// URL parameter.
    #[tokio::test]
    async fn a_url_parameter_on_a_body_only_binding_is_invalid_request() {
        let rejection = AdmittedTemporalPostQuery::<StandInGate, EmptyParams>::from_request(post("/ngsi-ld/v1/temporal/entityMaps?limit=3", WINDOWED), &())
            .await
            .err()
            .expect("a URL parameter is refused");
        pretty_assertions::assert_eq!(rejection.problem().map(ProblemDetail::error_type), Some(NgsiLdErrorType::InvalidRequest));
    }

    /// The body is admitted by the same route admission the query binding
    /// runs, so what the gate refuses is refused there too.
    #[tokio::test]
    async fn the_admission_runs_over_a_body_only_binding() {
        let context = AdmittedContext::core_only();
        let admitted = AdmittedTemporalPostQuery::<StandInGate, EmptyParams>::from_request(post("/ngsi-ld/v1/temporal/entityMaps", WINDOWED), &())
            .await
            .map_err(|_| "the body decodes")
            .expect("decodes");
        assert!(admitted.params.admitted(&context).await.is_ok());
        let filtered = AdmittedTemporalPostQuery::<StandInGate, EmptyParams>::from_request(post("/ngsi-ld/v1/temporal/entityMaps", FILTERED), &())
            .await
            .map_err(|_| "the body decodes")
            .expect("decodes");
        let refused = filtered.params.admitted(&context).await.expect_err("the admission refuses it");
        pretty_assertions::assert_eq!(refused.problem().error_type(), NgsiLdErrorType::InvalidRequest);
    }
}
