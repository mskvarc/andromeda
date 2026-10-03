//! The paged collection both subscription families answer a listing with
//! (clauses 5.8.4 and 5.11.4).
//!
//! `GET /subscriptions` and `GET /csourceSubscriptions` read the same
//! parameters and answer with the same collection shape; only the service that
//! lists them differs. The page they ask for and the response they answer with
//! are therefore decided here, once.

use crate::error::{SubscriptionResultExt, internal_problem};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    internal_failure_summary::InternalFailureSummary,
    pagination::{PageOutcome, append_page_links},
    problem_response::ProblemResponse,
    rendered_response::RenderedResponse,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow};
use andromeda_ngsi_ld_request::{
    options::ListOptions,
    pagination::{
        page_pointers::PagePointers,
        page_request::{CollectionOrder, PageRequest},
        policy::PaginationPolicy,
    },
    subscription_listing::{ListParams, SUBSCRIPTION_PAGE_SIZE},
};
use axum::{
    http::Uri,
    response::{IntoResponse, Response},
};

/// The page a subscription listing asks for, and the options its resources
/// are rendered under.
pub(crate) struct ListingRequest {
    /// The page the pagination policy decided.
    pub(crate) window: PageWindow,
    /// The `count` and `sysAttrs` options (clauses 6.3.11 and 6.3.13).
    pub(crate) options: ListOptions,
}

impl ListingRequest {
    /// The page `params` asks for under `pagination`.
    ///
    /// The subscriptions are listed in identifier order in either pagination
    /// mode, so a cursor can always walk them.
    ///
    /// # Errors
    ///
    /// Returns the `InvalidRequest` of an `options` value clauses 6.3.11 and
    /// 6.3.13 do not define, and whatever the pagination policy refuses.
    pub(crate) fn of(pagination: PaginationPolicy, params: &ListParams, instance: &Uri) -> Result<ListingRequest, ProblemResponse> {
        let options = params.list_options().with_subscription_instance(instance)?;
        let window = pagination
            .window(
                &params.page,
                PageRequest {
                    size: SUBSCRIPTION_PAGE_SIZE,
                    order: CollectionOrder::Identifier,
                    count: options.count,
                    pointers: PagePointers::OverRequestUri,
                },
            )
            .with_subscription_instance(instance)?;
        Ok(ListingRequest { window, options })
    }

    /// The collection response of a rendered page: the clause 6.3.13 count
    /// when the request asked for it, and the clause 6.3.10 pointers.
    ///
    /// # Errors
    ///
    /// Returns an internal error when the stored total does not fit the
    /// response platform, which no client input can provoke.
    pub(crate) fn respond(
        &self,
        rendered: impl Into<RenderedResponse>,
        total: i64,
        boundary: Option<(Identifier, Identifier)>,
        returned: usize,
        instance: &Uri,
        media_type: ResponseMediaType,
    ) -> Result<Response, ProblemResponse> {
        let mut response = NgsiLdCollectionResponse::ok(rendered);
        match self.options.count {
            CountPolicy::Emit => {
                // The store counts rows, so the total is never negative; a
                // conversion that fails here is an internal inconsistency.
                let total_count =
                    usize::try_from(total).map_err(|error| internal_problem(&error, InternalFailureSummary::new("internal subscription error"), instance))?;
                response = response.with_total_count(total_count);
            }
            CountPolicy::Omit => {}
        }
        let mut response = response.into_response();
        append_page_links(
            &mut response,
            instance,
            &self.window,
            PageOutcome {
                total: Some(total),
                returned,
                boundary,
            },
            media_type,
        );
        Ok(response)
    }
}
