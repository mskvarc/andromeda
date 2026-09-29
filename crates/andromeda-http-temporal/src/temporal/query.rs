//! HTTP handling for temporal query (clause 5.7.4).
//!
//! The operation has two HTTP bindings. Clause 6.18.3.2 carries the query in a
//! query string, clause 6.24.3.1 carries it in a clause 5.2.23 body, and clause
//! 6.24.1 says of the latter that "the behaviour of this clause mirrors the one
//! in clause 6.18.3.2". They therefore differ only in how the query reaches the
//! broker: once it has been decoded, [`run`] is the operation, and both
//! bindings hand it the same [`TemporalQueryRequest`].

use crate::{
    grouping_gate::reject_grouping_when_disabled,
    history_read::TemporalHistoryRead,
    join::apply_temporal_join,
    request_context_admission::admit_request_context,
    state::TemporalRouteServices,
    temporal::{
        AcceptFormatNoGeo,
        ContextSpec,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        ProblemResponse,
        Response,
        ResultInstanceExt,
        StatusCode,
        TemporalQueryParameters,
        TenantState,
        ValidatedQuery,
        query::{
            complete_history::HistoryCompleteness,
            forwarding::{QueryScope, TemporalForwarding, forward_temporal_history},
            merged_history::merge_forwarded_history,
            merged_page::{MergedPageCut, cut_merged_page, defer_split_filters, hold_page_until_merge},
            page::QueriedPage,
            request_entity_map::{record_query_map, resolve_query_map},
            response::{TemporalCollectionMetadata, query_response, simplified_collection},
        },
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_federation::{
    FederationService,
    forwarded_query::client_query::ClientQuery,
    operation::ForwardingOperation,
    ordering_admission::{OrderedRead, OrderingAdmission},
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_http_extraction::{context_target, prefer::PreferHeader};
use andromeda_http_headers::{ngsild_warning::NgsiLdWarning, via::Via};
use andromeda_http_response::pagination::PagePointers;
use andromeda_ngsi_ld_lexicon::uri::Uri as NgsiLdUri;
use andromeda_ngsi_ld_request::{error::ParameterError, join_traversal::RetrievingJoin, pagination_cursor::decode_cursor};
use andromeda_temporal::{
    error::TemporalError,
    linked::candidates::TemporalLinkedCandidate,
    service::{TemporalQueryWarning, TemporalRange},
};
use axum::{
    Extension,
    http::{HeaderMap, Uri},
};
use serde_json::Value;
use std::sync::Arc;

/// How far the merged history is complete (clause 6.3.10).
mod complete_history;
/// Asking the registered context sources for their part of the history.
mod forwarding;
/// Folding what those sources returned into the locally read history.
mod merged_history;
/// The page the query answers with, cut from the merged history.
mod merged_page;
/// The clause 6.3.10 element page the response points around.
mod page;
/// The `EntityMap` the query runs under.
mod request_entity_map;
/// Rendering the collection the query answers with.
mod response;

/// One "Query Temporal Evolution of Entities" as its binding decoded it.
///
/// Everything here is what the operation needs and nothing is specific to the
/// binding that supplied it: the query string binding of clause 6.18.3.2 fills
/// [`TemporalQueryRequest::params`] from the URL, the POST binding of clause
/// 6.24.3.1 fills it from the clause 5.2.23 body, and from there on the two are
/// the same request.
pub(crate) struct TemporalQueryRequest {
    /// The tenant-scoped capabilities the query runs against.
    pub(crate) services: Arc<TemporalRouteServices>,
    /// The context branch the query reads.
    pub(crate) target: ContextTarget,
    /// The representation the response was negotiated to (clause 6.3.4).
    pub(crate) accept: AcceptFormatNoGeo,
    /// The clause 6.3.x protocol headers: the `NGSILD-EntityMap` the request
    /// runs under, the `Via` chain it arrived with, and the extension headers
    /// that add to the query.
    pub(crate) protocol: NgsiLdProtocolHeaders,
    /// The URI the client addressed, which names the `instance` of every
    /// problem the operation reports and anchors its pagination links.
    pub(crate) uri: Uri,
    /// The request's headers, replayed onto every forwarded copy so a
    /// registration's `contextSourceInfo` can pass one of them through
    /// (clause 6.3.19).
    pub(crate) headers: HeaderMap,
    /// The JSON-LD context the query's terms expand under (clause 6.3.5).
    pub(crate) context: ContextSpec,
    /// The query itself, in the raw form the binding decoded it to.
    pub(crate) params: TemporalQueryParameters,
    /// Whether the binding's own request URI can carry the clause 6.3.10
    /// pointers to the neighbouring pages, which the query-string binding's
    /// can and the POST binding's cannot.
    pub(crate) pointers: PagePointers,
}

pub async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: TenantState,
    accept: AcceptFormatNoGeo,
    Extension(enabled): Extension<EnabledExtensions>,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via: _,
        headers,
    }: NgsiLdRequestEnvelope,
    protocol: NgsiLdProtocolHeaders,
    ValidatedQuery(params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ProblemResponse> {
    // The grouped extension is refused upstream when it is disabled, so a
    // conformant broker rejects `aggrGroupBy`/`groupedValues` before the query
    // is admitted (extension-strategy 5.2).
    reject_grouping_when_disabled(&params, enabled, &uri)?;
    // Boxed: a temporal query parameter set is wide enough that the operation's
    // future overflows the task-local budget the `large_futures` lint guards.
    Box::pin(run(TemporalQueryRequest {
        services,
        target,
        accept,
        protocol,
        uri,
        headers,
        context: context.into_inner(),
        params,
        // Clause 6.18.3.2 carries the whole query in the request URI, so a
        // pointer built from it retrieves the next page on its own.
        pointers: PagePointers::OverRequestUri,
    }))
    .await
}

/// One temporal query, decoded and admitted: what the protocol headers
/// contributed to it, and the Entity Map it runs under.
struct AdmittedQuery {
    /// The `Prefer` header the response honours (clause 6.3.21).
    prefer: PreferHeader,
    /// The `Via` chain the request arrived with, replayed onto every forwarded
    /// copy so a forwarding loop is detected rather than followed.
    incoming_via: Option<Via>,
    /// The Entity Map the query runs under (clause 5.5.14).
    map_use: EntityMapUse,
}

/// Decode `protocol` into the query, hold the query to every rule that can be
/// decided before it runs, and resolve the Entity Map it runs under.
///
/// The extension headers add to the query before it is validated, so a value a
/// header supplied is held to the same rules as one the query string carried.
/// A branch that cannot federate is confined to the local scope first: clause
/// 5.5.15 makes a Snapshot "implicitly limited to a local scope ... overriding
/// setting a local scope to false".
///
/// # Errors
///
/// The classified failure of an extension header the query cannot take, of
/// resolving the Entity Map, of the clause 5.7.4.4 validation, and of the
/// clause 5.7.4.4 ordering rule.
async fn admit_request(
    state: &TemporalRouteServices,
    target: &ContextTarget,
    context: &ContextSpec,
    protocol: NgsiLdProtocolHeaders,
    params: &mut TemporalQueryParameters,
    instance: &Uri,
) -> Result<AdmittedQuery, ProblemResponse> {
    protocol.extensions().apply_to_temporal_query(params).with_instance(instance)?;
    let map_use = resolve_query_map(&state.entity_maps, target, params, protocol.entity_map.as_ref(), instance).await?;
    if !target.permits_federation() {
        params.local = Some(true);
    }
    params.validate(true).with_instance(instance)?;
    refuse_inadmissible_ordering(&state.forwarding, target, context, params, EntityMapInUse::from(&map_use), instance).await?;
    Ok(AdmittedQuery {
        prefer: protocol.prefer,
        incoming_via: protocol.incoming_via,
        map_use,
    })
}

/// Refuse an ordering clause 5.7.4.4 does not admit.
///
/// The bullet raises `BadRequestData` for an `orderBy` on a query "not limited
/// to the local scope", which `andromeda_federation::ordering_admission` reads
/// as an answer a registered Context Source takes part in assembling. That is a
/// question for the Context Registry rather than for the parameters, so it is
/// asked here rather than during validation. Which member may order a temporal
/// query is a separate rule the parameters do settle, and validation keeps it.
///
/// # Errors
///
/// The classified failure of reading the registrations that would take part,
/// and the clause 5.7.4.4 refusal itself when one of them does.
async fn refuse_inadmissible_ordering(
    forwarding: &FederationService,
    target: &ContextTarget,
    context: &ContextSpec,
    params: &TemporalQueryParameters,
    entity_map: EntityMapInUse<'_>,
    instance: &Uri,
) -> Result<(), ProblemResponse> {
    let admission = forwarding
        .admit_ordering(
            target,
            context,
            OrderedRead {
                ordering: &params.ordering,
                requested_scope: params.execution_scope(),
                client_query: ClientQuery::Temporal(params),
                operation: ForwardingOperation::QueryTemporalEntity,
                entity_map,
            },
        )
        .await
        .with_instance(instance)?;
    match admission {
        OrderingAdmission::Admitted => Ok(()),
        OrderingAdmission::Refused => Err(ParameterError::OrderingWithParticipatingSource).with_instance(instance),
    }
}

/// Run "Query Temporal Evolution of Entities" (clause 5.7.4) over a decoded
/// request, whichever binding decoded it.
///
/// # Errors
///
/// The classified failure of resolving the Entity Map the query runs under, of
/// reading the local history, of merging what the context sources contributed,
/// of walking the clause 4.5.23 join, or of rendering the negotiated
/// representation; and the Table 6.4.3.2-3 `NotImplemented` answer when the one
/// source holding all of the data declined that representation.
pub(crate) async fn run(request: TemporalQueryRequest) -> Result<Response, ProblemResponse> {
    let TemporalQueryRequest {
        services: state,
        target,
        accept,
        protocol,
        uri,
        headers,
        context,
        mut params,
        pointers,
    } = request;
    let service = &state.temporal;
    // Clause 5.5.7: a request `@context` declaring a JSON-LD Scoped Context is
    // refused on the temporal query path -- GET and POST alike -- before it is
    // forwarded or expanded against.
    admit_request_context(service.engine(), &context, &uri).await?;
    let AdmittedQuery { prefer, incoming_via, map_use } = admit_request(&state, &target, &context, protocol, &mut params, &uri).await?;
    let key_values = params.is_simplified();

    // Clause 5.5.9.1: decode the opaque pagination cursor before the page is
    // planned, so a malformed one is a BadRequestData here rather than surfacing
    // from the store. Its mutual exclusion with offset was already held by
    // validation.
    let incoming_cursor = decode_cursor(params.cursor.as_deref()).with_instance(&uri)?;
    // Clause 6.3.10: the page the client asked for, read before the merge
    // widens the local query below, so the links point at the client's own
    // window rather than at the widened one.
    let page = QueriedPage::of(&params, pointers, incoming_cursor);

    let scope = QueryScope::of(&target, &params);

    let split = state.forwarding.split_entities(params.split_entities);
    let filtering = scope.splitting(split);

    // Forwarding reads the query as the client wrote it, so it runs before the
    // split-entities filters are held back from the local query below.
    let forwarded = forward_temporal_history(
        TemporalForwarding {
            forwarding: &state.forwarding,
            triggering: TriggeringRequest {
                via: incoming_via.as_ref(),
                headers: Some(&headers),
            },
            split,
            map_use: &map_use,
        },
        scope,
        service.engine(),
        &target,
        &params,
        &context,
        &uri,
    )
    .await?;

    let deferred = defer_split_filters(&mut params, filtering);
    let merged_page_window = hold_page_until_merge(&mut params, forwarded.as_ref(), &page);

    let local_result = service.query(&target, &params, &context).await.with_instance(&uri)?;
    log_query_warnings(&local_result.warnings);
    let local_total = local_match_count(local_result.total_count).with_instance(&uri)?;
    let completeness = HistoryCompleteness::of(&params, local_result.range, forwarded.as_ref()).with_instance(&uri)?;
    let mut merged = merge_forwarded_history(local_result.entities, forwarded, filtering, &uri)?;
    let timeproperty = params.timeproperty_or_default();
    completeness.trim(&mut merged.entities, timeproperty);

    let (mut entities, total_count) = cut_merged_page(
        service.engine(),
        &context,
        merged.entities,
        MergedPageCut {
            deferred,
            filtering,
            window: merged_page_window,
            local_total,
        },
        &params,
        &uri,
    )
    .await?;
    // Clause 6.3.10: the `Content-Range` describes the history the page's own
    // entities carry, read before a flat join appends the linked ones.
    let range = completeness.range(&entities, timeproperty);
    // Clause 6.3.10 pages the matching entities; a flat join appends the
    // entities they link to, which accompany the page rather than filling it.
    let returned = entities.len();
    // Clause 5.5.9.1: the boundary a cursor pointer walks from is the first and
    // last identifiers of the matching page, captured here before the flat join
    // appends the linked entities the page must not point past.
    let page_boundary = element_boundary(&entities);

    // Join mode (clause 4.5.23) is parsed at the query boundary. `@none` and an
    // absent `join` retrieve no linked entity at all, so neither reaches the
    // traversal. The join runs after forwarding and after the deferred
    // `splitEntities` filtering, so the roots it walks are the ones the client
    // will actually be shown.
    // Clause 6.3.13 counts the matches the query returned, which the linked
    // entities are not: they accompany the result set rather than belonging to
    // it. Counting before the flat join appends is what keeps `count` and the
    // 206 decision about the roots alone. The join also resolves before
    // EntityMap creation so a requested map can include the linked ids
    // (spec 5.5.14, 5.7.4.4).
    let read = TemporalHistoryRead {
        service: &state.temporal,
        target: &target,
        params: &params,
        context: &context,
    };
    let (warnings, linked_ids) = join_linked_entities(&read, &mut entities, &local_result.linked_candidates, merged.warnings, &uri).await?;

    let map_created = map_use.mints();
    let entity_map_uri = record_query_map(&state.entity_maps, &read, map_use, merged.contributions, linked_ids, &uri).await?;

    let values = if key_values { simplified_collection(entities, &uri)? } else { entities };

    let status = response_status(range.as_ref(), map_created);
    // Table 6.18.3.2-2 carries the 203 row and clause 6.3.6 the
    // `Preference-Applied` echo, so the preference the request expressed is
    // what renders; clause 6.3.12's representation is what decides how clause
    // 4.3.6.8 reads an Attribute of the answer.
    query_response(
        accept,
        prefer,
        params.selected_format(),
        &context,
        values,
        TemporalCollectionMetadata {
            status,
            warnings,
            total_count,
            range,
            entity_map_uri,
            page,
            returned,
            boundary: page_boundary,
        },
        &uri,
    )
}

/// The status a temporal query answers with (Table 6.18.3.2-2 and Table
/// 6.24.3.1-1): 206 when the history -- the local read's or a context
/// source's -- was cut short at `range`, else 201 when an `EntityMap` has been
/// (re)created, else 200.
const fn response_status(range: Option<&TemporalRange>, map_created: bool) -> StatusCode {
    if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else if map_created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    }
}

/// The first and last entity identifiers of the matching element page, from
/// which a clause 5.5.9.1 cursor pointer is computed.
///
/// Absent when the page is empty or when an entity carries no readable `id`, in
/// which case the cursor pointers are not offered and the offset path stands.
fn element_boundary(entities: &[Value]) -> Option<(NgsiLdUri, NgsiLdUri)> {
    let id_of = |entity: &Value| entity.get("id").and_then(Value::as_str).and_then(|id| id.parse::<NgsiLdUri>().ok());
    let first = entities.first().and_then(&id_of)?;
    let last = entities.last().and_then(&id_of)?;
    Some((first, last))
}

/// Record the non-fatal per-entity failures a temporal query collected.
fn log_query_warnings(warnings: &[TemporalQueryWarning]) {
    for warning in warnings {
        tracing::debug!(entity_id = %warning.entity_id, message = %warning.message, "Temporal query warning");
    }
}

/// The number of matches the local read reported, in the width the
/// `NGSILD-Results-Count` header carries.
///
/// # Errors
///
/// [`TemporalError::Internal`] when the store reported a negative count, which
/// counting rows cannot produce.
fn local_match_count(total: Option<i64>) -> Result<Option<usize>, TemporalError> {
    total
        .map(|count| usize::try_from(count).map_err(|_| TemporalError::internal("the local temporal read reported a negative match count")))
        .transpose()
}

/// Walk the join (clause 4.5.23) the query asked for, appending or inlining the
/// linked entities and reporting what the traversal could not reach.
///
/// `@none` and an absent `join` retrieve nothing, so neither reaches the
/// traversal and the merged warnings pass through untouched.
///
/// # Errors
///
/// The classified failure when the linked-entity query cannot be compiled or
/// the base entity store cannot be read.
async fn join_linked_entities(
    read: &TemporalHistoryRead<'_>,
    entities: &mut Vec<Value>,
    seed: &[TemporalLinkedCandidate],
    warnings: Vec<NgsiLdWarning>,
    instance: &Uri,
) -> Result<(Vec<NgsiLdWarning>, Vec<NgsiLdUri>), ProblemResponse> {
    let Some(join) = RetrievingJoin::from_mode(read.params.join) else {
        return Ok((warnings, Vec::new()));
    };
    let applied = apply_temporal_join(read, join, entities, seed, warnings).await.with_instance(instance)?;
    Ok((applied.warnings, applied.linked_ids))
}
