//! The forwarding of a single-entity attribute write, which `local=true`
//! withholds.
//!
//! Clause 6.3.18: "If local=true then no Context Source Registrations shall be
//! considered as matching". A local write therefore plans no forwarding and
//! sends its payload to no source, while the plan-then-execute order the
//! distributed write keeps (the clause 5.5.4 validation runs between the two)
//! stays the same for both.

use crate::error::EntityResultExt;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{
    FederationService,
    forwarded_content::ForwardedContent,
    outcome::ForwardedWriteResult,
    provision_plan::ProvisionPlan,
    request::ForwardProvisionRequest,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_request::query_narrowing::ExecutionScope;
use axum::http::Uri as HttpUri;

/// The forwarding one attribute write runs, or none for a local write.
pub(crate) struct ScopedProvision(Option<ProvisionPlan>);

impl ScopedProvision {
    /// Plan the forwarding of `request` unless `scope` confines it here.
    ///
    /// # Errors
    ///
    /// Returns the problem the planning failure answers with.
    pub(crate) async fn plan(
        forwarding: &FederationService,
        target: &ContextTarget,
        request: &ForwardProvisionRequest<'_>,
        scope: ExecutionScope,
        instance: &HttpUri,
    ) -> Result<ScopedProvision, ProblemResponse> {
        match scope {
            ExecutionScope::Local => Ok(ScopedProvision(None)),
            ExecutionScope::Distributed => Ok(ScopedProvision(Some(
                forwarding.plan_provision(target, request).await.with_entity_instance(instance)?,
            ))),
        }
    }

    /// Whether a registration reads the write's payload.
    pub(crate) const fn reads_payload(&self) -> bool {
        match &self.0 {
            Some(plan) => plan.reads_payload(),
            None => false,
        }
    }

    /// Send the write to the planned registrations; a local write sends it to
    /// none.
    ///
    /// # Errors
    ///
    /// Returns the problem the forwarding failure answers with.
    pub(crate) async fn execute(
        self,
        forwarding: &FederationService,
        request: &ForwardProvisionRequest<'_>,
        content: Option<ForwardedContent<'_>>,
        instance: &HttpUri,
    ) -> Result<Vec<ForwardedWriteResult>, ProblemResponse> {
        match self.0 {
            Some(plan) => forwarding.execute_provision(plan, request, content).await.with_entity_instance(instance),
            None => Ok(Vec::new()),
        }
    }
}
