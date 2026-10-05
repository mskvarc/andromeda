//! The temporal query a request carries, extracted already past the broker's
//! own admission of it.
//!
//! The aggregation-bearing temporal query bindings -- `GET /temporal/entities`,
//! `GET /temporal/entities/{id}` and `POST /temporal/entityOperations/query` --
//! read their parameters and then refuse a query the broker does not enable
//! (an extension that is switched off must behave as one never implemented).
//! Extracting the parameters through these types makes that refusal part of
//! reading them, so no binding can read the parameters and forget it. The
//! admission itself is the route's: it is installed as a request extension of
//! the [`TemporalQueryAdmission`] type the binding names.

use crate::{addressed_uri::addressed_uri, jsonld::JsonLd, query::ValidatedQuery};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    extraction_rejection::ExtractorRejection,
    internal_failure_summary::InternalFailureSummary,
    parameter_failure::classify_parameter_error,
};
use andromeda_jsonld::user_context_admission::SubmittedContext;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_request::query::temporal::{TemporalQueryParameters, post_parameters::TemporalPostQueryParams};
use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    http::request::Parts,
};
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

/// The URL parameters of a GET temporal query (clauses 6.18.3.2 and 6.19.3.1),
/// admitted by the route's `A`.
pub struct AdmittedTemporalQuery<A> {
    /// The admitted parameters.
    pub params: TemporalQueryParameters,
    /// The admission the parameters passed.
    admission: PhantomData<A>,
}

impl<S, A> FromRequestParts<S> for AdmittedTemporalQuery<A>
where
    S: Send + Sync,
    A: TemporalQueryAdmission,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<AdmittedTemporalQuery<A>, ExtractorRejection> {
        let ValidatedQuery(params) = ValidatedQuery::<TemporalQueryParameters>::from_request_parts(parts, state).await?;
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

/// The clause 5.2.23 body and URL parameters of a POST temporal query (clause
/// 6.24.3.1), folded into one parameter set admitted by the route's `A`, with
/// the one context the body extractor resolved (clause 6.3.5).
pub struct AdmittedTemporalPostQuery<A> {
    /// The query the body and the URL parameters describe.
    pub params: TemporalQueryParameters,
    /// The context the query is read against: the body's own `@context` for
    /// `application/ld+json`, the `Link` context for `application/json`.
    pub context: SubmittedContext,
    /// The admission the parameters passed.
    admission: PhantomData<A>,
}

impl<S, A> FromRequest<S> for AdmittedTemporalPostQuery<A>
where
    S: Send + Sync,
    A: TemporalQueryAdmission,
{
    type Rejection = ExtractorRejection;

    async fn from_request(request: Request, state: &S) -> Result<AdmittedTemporalPostQuery<A>, ExtractorRejection> {
        let (mut parts, body) = request.into_parts();
        let ValidatedQuery(url_params) = ValidatedQuery::<TemporalPostQueryParams>::from_request_parts(&mut parts, state).await?;
        let admission = installed_admission::<A>(&parts)?;
        let uri = addressed_uri(&parts);
        let JsonLd { value: query, context } = JsonLd::<Query>::from_request(Request::from_parts(parts, body), state).await?;
        let mut params = TemporalQueryParameters::try_from(query)
            .map_err(|error| ExtractorRejection::Classified(classify_parameter_error(&error).with_instance(uri.clone())))?; // clone: the admission below names the request too
        url_params.apply_to(&mut params);
        // The body's `aggrParams.aggrGroupBy` is folded into `params` by now,
        // so the admission sees the whole query.
        admission
            .admit(&params, TemporalQueryBinding::Body)
            .map_err(|report| ExtractorRejection::Classified(report.with_instance(uri)))?;
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
