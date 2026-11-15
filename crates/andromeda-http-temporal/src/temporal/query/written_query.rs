//! The temporal query as the client wrote it, compiled once and only when
//! something reads it.
//!
//! A walk over the request's URI is paged under the identity of the written
//! query (clause 5.5.9.1), and an Entity Map is fingerprinted by it (clause
//! 5.5.14), both before an Entity Map narrows its identifiers and before split
//! entities defer its filters. A query read by neither, a posted walk under no
//! map, is never compiled as written.

use crate::temporal::{ContextSpec, ProblemResponse, ResultInstanceExt, TemporalQueryParameters, query::request_entity_map::open_window};
use andromeda_ngsi_ld_query::{projection::attrs_selection::AttrsAdmission, temporal::compiled::CompiledTemporalQuery};
use andromeda_ngsi_ld_request::query::temporal::operation::TemporalQueryOperation;
use andromeda_temporal::{query_reading::QueryReading, service::TemporalService};
use axum::http::Uri;
use std::sync::OnceLock;

/// The query as the client wrote it, compiled under the broker's own language
/// targeting on first use.
pub(super) struct WrittenQuery<'a> {
    /// The service the query compiles in.
    service: &'a TemporalService,
    /// The `@context` the query's terms expand under.
    context: &'a ContextSpec,
    /// The parameters as the client wrote them, before an Entity Map narrows
    /// the request's own.
    params: TemporalQueryParameters,
    /// The request a failure is reported against.
    instance: &'a Uri,
    /// The compiled query, once something read it.
    compiled: OnceLock<CompiledTemporalQuery>,
}

impl<'a> WrittenQuery<'a> {
    /// The query `params` hold as the client wrote them.
    pub(super) const fn new(service: &'a TemporalService, context: &'a ContextSpec, params: TemporalQueryParameters, instance: &'a Uri) -> WrittenQuery<'a> {
        WrittenQuery {
            service,
            context,
            params,
            instance,
            compiled: OnceLock::new(),
        }
    }

    /// The compiled query, compiled on the first call.
    ///
    /// A follow-up under an Entity Map may omit the temporal window, which the
    /// identity does not carry, so an omitted one is stood for the open window
    /// the compilation needs.
    ///
    /// # Errors
    ///
    /// The classified failure of admitting and compiling the query.
    pub(super) async fn compiled(&self) -> Result<&CompiledTemporalQuery, ProblemResponse> {
        if let Some(compiled) = self.compiled.get() {
            return Ok(compiled);
        }
        let compiled = self.compile().await?;
        Ok(self.compiled.get_or_init(|| compiled))
    }

    /// Admit and compile the written parameters.
    async fn compile(&self) -> Result<CompiledTemporalQuery, ProblemResponse> {
        // clone: the window is opened on the compiled copy, while the parameters stay as written
        let mut written = self.params.clone();
        open_window(&mut written).with_instance(self.instance)?;
        let admitted = written
            .admit(TemporalQueryOperation::Query, self.service.linked_retrieval(), self.service.filter_admission())
            .with_instance(self.instance)?;
        self.service
            .compile_temporal_query(
                &admitted,
                QueryReading {
                    context: self.context,
                    language_targeting: self.service.language_targeting(),
                },
                AttrsAdmission::ReifiedScopeAdmitted,
            )
            .await
            .with_instance(self.instance)
    }
}
