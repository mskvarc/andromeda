//! The query-string binding of Query Temporal Evolution of Entities (clause
//! 6.18.3.2): the query a client wrote, or a followed pointer naming a query a
//! client sent by POST.

use crate::{
    admitted_temporal_query::{AdmittedTemporalQuery, TemporalQueryAdmission, installed_admission},
    posted_reference::{decode_posted_reference, names_posted_query},
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_request::{pagination::posted_reference::PostedQueryReference, query::temporal::post_parameters::TemporalPostQueryParams};
use axum::{extract::FromRequestParts, http::request::Parts};

/// What the query string of `GET /temporal/entities` names, under the
/// route's admission `A`.
pub enum TemporalQuerySource<A> {
    /// The query itself, admitted.
    Query(Box<AdmittedTemporalQuery<A>>),
    /// A posted query the broker holds, by its key, beside the URL parameters
    /// of the POST that posted it (clause 6.24.3.1). The held query is
    /// admitted by `admission` once it is read back.
    Posted {
        /// The key and the URL parameters the pointer carries.
        reference: PostedQueryReference<TemporalPostQueryParams>,
        /// The admission the route installed.
        admission: A,
    },
}

impl<S, A> FromRequestParts<S> for TemporalQuerySource<A>
where
    S: Send + Sync,
    A: TemporalQueryAdmission,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<TemporalQuerySource<A>, ExtractorRejection> {
        if names_posted_query::<TemporalPostQueryParams>(parts) {
            return Ok(TemporalQuerySource::Posted {
                reference: decode_posted_reference(parts)?,
                admission: installed_admission::<A>(parts)?,
            });
        }
        AdmittedTemporalQuery::from_request_parts(parts, state)
            .await
            .map(|query| TemporalQuerySource::Query(Box::new(query)))
    }
}
