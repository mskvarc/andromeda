//! Which registrations a single-entity provision operation is forwarded to,
//! decided before its payload is read (clauses 4.3.6, 5.12, 6.3.17 and
//! 6.3.18).
//!
//! Matching a write to registrations needs the entity identifier, its types
//! and the attribute the request path names, never the payload. A write
//! handler therefore plans its forwarding first and has its payload compacted
//! and written only for a registration that reads it: a write no registration
//! covers -- the common case -- keeps its payload for the local write alone.

use crate::{
    FederationService,
    error::FederationError,
    federation_invariant::FederationInvariant,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    provision::{is_proxied, proxied_mode},
    registration_information::{csr_has_matching_information, csr_matches_selected_entity},
    request::ForwardProvisionRequest,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::term_expansion::ContextualExpander;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_ngsi_ld_query::type_selection::{expansion::expand_type_expression, grammar::TypeExprRaw};
use andromeda_telemetry::{
    federation_ops::{self, FederationOperation},
    operation::OperationTimer,
};
use iri_rs::IriBuf;

/// The registrations one provision operation is forwarded to.
pub struct ProvisionPlan {
    /// The measurement of the whole forwarding, started when it is planned
    /// and finished when it is executed.
    pub(crate) timer: OperationTimer,
    /// The registrations the operation matched.
    pub(crate) selection: RegistrationSelection,
}

/// The registrations one provision operation matched, by what each does with
/// it.
#[derive(Default)]
pub(crate) struct RegistrationSelection {
    /// Outcomes decided without contacting a source: a proxied registration a
    /// looped-back request cannot be forwarded to (clause 6.3.17).
    pub(crate) settled: Vec<ForwardedWriteResult>,
    /// The registrations the write is sent to.
    pub(crate) receiving: Vec<ContextSourceRegistration>,
    /// The proxied registrations that do not support the operation but still
    /// own their attributes (clauses 5.6.1.4 and following).
    pub(crate) not_advertising: Vec<ContextSourceRegistration>,
    /// The entity types the registrations were matched on.
    pub(crate) entity_types: Vec<IriBuf>,
    /// The attribute the request path names, by its expanded name.
    pub(crate) expanded_attribute: Option<AttributeName>,
}

impl ProvisionPlan {
    /// Whether a registration reads the operation's payload.
    #[must_use]
    pub const fn reads_payload(&self) -> bool {
        !self.selection.receiving.is_empty() || !self.selection.not_advertising.is_empty()
    }
}

impl FederationService {
    /// Decide which registrations a single NGSI-LD provision operation is
    /// forwarded to, without reading its payload.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the request's entity type or
    /// attribute cannot be expanded against its context, and
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read.
    pub async fn plan_provision(&self, target: &ContextTarget, request: &ForwardProvisionRequest<'_>) -> Result<ProvisionPlan, FederationError> {
        let timer = federation_ops::start(FederationOperation::Provision);
        match self.select_provision_registrations(target, request).await {
            Ok(selection) => Ok(ProvisionPlan { timer, selection }),
            Err(error) => {
                let failed: Result<(), &FederationError> = Err(&error);
                timer.finish_result(&failed);
                Err(error)
            }
        }
    }

    /// The registrations `request` matches.
    async fn select_provision_registrations(
        &self,
        target: &ContextTarget,
        request: &ForwardProvisionRequest<'_>,
    ) -> Result<RegistrationSelection, FederationError> {
        if !target.permits_federation() {
            return Ok(RegistrationSelection::default());
        }
        // Clause 6.3.18: an incoming request whose Via already names this
        // broker's own hostAlias has looped back onto it. The forward is not
        // sent; how the operation answers depends on the matched registration
        // (clause 6.3.17), decided per registration below.
        let looped = self.is_loop_detected(request.triggering.via);

        let resolver = TermResolver::new(&self.engine, request.context);
        let (entity_types, type_selector): (Vec<IriBuf>, Option<TypeExprRaw>) = match request.entity_type_selector {
            // Clause 5.12 holds the selector against each registration's types.
            // The flat list the rest of the forwarding reads is the types the
            // selector names.
            Some(raw) => {
                let expanded = expand_type_expression(raw, &ContextualExpander::new(&self.engine, resolver.prepared())).await?;
                (expanded.named_types(), Some(expanded.into_raw()?))
            }
            // A write that carries no `type` in its request is matched against
            // registrations by the local entity's actual types -- every one it
            // carries, since clause 5.12 matches on any of them -- so a
            // registration scoped to an unrelated type is not taken to cover
            // the write and refuse it. A write to an entity this broker holds
            // only at a source resolves to an empty set and falls back to
            // id-only matching, as before.
            None => (self.local_entity_types.local_entity_types(target, request.entity_id).await, None),
        };
        // The path attribute is a compact term; matching compares IRIs, so
        // it is expanded once against the triggering request's context.
        let expanded_attribute = match request.attribute_name {
            Some(attribute) => Some(resolver.expand_attr(attribute.as_str()).await?),
            None => None,
        };
        let expanded_name = expanded_attribute
            .as_ref()
            .map(|iri| iri.as_str().parse::<AttributeName>())
            .transpose()
            .map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))?;

        // The store pre-filter is a coarse superset; clause-5.12 precision is
        // applied by csr_has_matching_information below. A single known type
        // narrows the query, a selector only when it is one bare type; zero or
        // several types take the whole id-covering superset and let the precise
        // match do the filtering.
        let coarse_type = match type_selector.as_ref() {
            Some(selector) => selector.as_single_type().and(entity_types.first()),
            None => match entity_types.as_slice() {
                [single] => Some(single),
                _ => None,
            },
        };
        let csrs = self.csr_repo.find_matching(coarse_type, Some(request.entity_id)).await?;

        // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
        let mut settled = Vec::new();
        let mut receiving = Vec::new();
        let mut not_advertising = Vec::new();
        for csr in csrs {
            if csr.access.mode == Some(ForwardingMode::Auxiliary) {
                continue;
            }
            let matches = match type_selector.as_ref() {
                Some(selector) => csr_matches_selected_entity(&csr, selector, request.entity_id, expanded_name.as_slice()),
                None => csr_has_matching_information(&csr, &entity_types, Some(request.entity_id), expanded_attribute.as_ref()),
            };
            if !matches {
                continue;
            }
            if looped {
                // Clause 6.3.17: with the request looped back, a proxied source
                // that holds all the data externally cannot be forwarded to, so
                // a lone one is 508 Loop Detected (`single_source_failure`
                // escalates it). An inclusive registration holds local data, so
                // the loop only stops the forward and the local part still runs.
                if is_proxied(&csr) {
                    let mode = proxied_mode(&csr);
                    settled.push(ForwardedWriteResult {
                        registration_id: csr.id,
                        mode,
                        outcome: ForwardedWriteOutcome::LoopDetected,
                        proxied_attrs: Vec::new(),
                    });
                }
                continue;
            }
            if request.operation.matches(&csr) {
                receiving.push(csr);
            } else if is_proxied(&csr) {
                not_advertising.push(csr);
            }
        }
        Ok(RegistrationSelection {
            settled,
            receiving,
            not_advertising,
            entity_types,
            expanded_attribute: expanded_name,
        })
    }
}
