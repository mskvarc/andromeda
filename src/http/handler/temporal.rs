//! API handlers for the NGSI-LD Temporal API.

use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    error::{AndromedaError, ResultInstanceExt},
    http::{
        error::problem::NgsiLdError,
        extractor::{
            accept::AcceptFormatNoGeo,
            context::ContextUrl,
            jsonld::JsonLd,
            prefer::PreferHeader,
            protocol::NgsiLdProtocolHeaders,
            query::ValidatedQuery,
        },
        response::{collection::NgsiLdCollectionResponse, render::start_rendering, single::NgsiLdResponse},
        validation::{parse_entity_id, validate_attr_name, validate_instance_id},
    },
    service::{
        csource_forwarding::{
            entity_result::collect_temporal_warnings,
            operation::ForwardingOperation,
            request::{CsrModeFilter, ForwardTemporalQueryRequest},
            temporal_result::merge_temporal_retrieve_results,
        },
        geo_filter::matches_geo_query,
        subscription_matching::matches_scope_query,
        temporal::{CreateOrUpdateResult, TemporalQueryResult, TemporalService},
    },
    temporal_render::simplify::{TemporalSimplifyPayload, TemporalSimplifyRenderer},
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{time_rel::TimeRel, uri::Uri};
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use andromeda_ngsi_ld_query::{
    filter::{eval::evaluate, expansion::expand_q_expr},
    geo::compile::compile_geo_query,
};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{
    attribute_deletion::DeleteAttrParams,
    deferred_split_filters::DeferredSplitFilters,
    empty::EmptyParams,
    query::temporal::TemporalQueryParameters,
};
use andromeda_postgresql_temporal_store::params::DeleteScope;
use axum::{
    extract::{OriginalUri, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::str::FromStr;

/// Temporal add attributes handling.
pub mod add_attributes;
/// Handles `POST /ngsi-ld/v1/temporal/entities`.
///
/// Creates a new temporal entity or updates an existing one from the request payload.
/// Temporal create or update handling.
pub mod create_or_update;
/// Temporal delete handling.
pub mod delete;
/// Temporal delete attribute handling.
pub mod delete_attribute;
/// Temporal delete attribute instance handling.
pub mod delete_attribute_instance;
/// Temporal query handling.
pub mod query;
/// Temporal retrieve handling.
pub mod retrieve;
/// Temporal update attribute instance handling.
pub mod update_attribute_instance;
