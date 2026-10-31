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
    provision::is_proxied,
    request::ForwardProvisionRequest,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::term_expansion::ContextualExpander;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::{
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
    registration_matching::{
        entity_specification::{EntitySpecification, TypeConstraint},
        match_specification::RegistrationMatchSpecification,
    },
};
use andromeda_ngsi_ld_query::{
    csr::expanded_entity_specification::ExpandedEntitySpecification,
    type_selection::{
        expansion::expand_type_expression,
        grammar::{TypeExpr, TypeExprRaw},
    },
};
use andromeda_telemetry::{
    federation_ops::{self, FederationOperation},
    operation::OperationTimer,
};
use iri_rs::IriBuf;
use std::slice::from_ref;

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
    /// The proxied registrations a looped-back request cannot be forwarded to
    /// (clause 6.3.17), which still own their attributes (clause 4.3.6.3).
    pub(crate) looped: Vec<ContextSourceRegistration>,
    /// The registrations the write is sent to.
    pub(crate) receiving: Vec<ContextSourceRegistration>,
    /// The proxied registrations that do not support the operation but still
    /// own their attributes (clauses 5.6.1.4 and following).
    pub(crate) not_advertising: Vec<ContextSourceRegistration>,
    /// The entity types the registrations were matched on.
    pub(crate) entity_types: Vec<EntityTypeName>,
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
        // Clause 5.12 holds a selector against each registration's types; the
        // flat list the rest of the forwarding reads is the types it names. The
        // store pre-filter is a coarse superset the match below decides: one
        // bare selected type, or the one type a local entity carries, narrows
        // it, and anything else takes the whole id-covering superset.
        let (entity_types, type_selector, coarse_type): (Vec<EntityTypeName>, Option<TypeExprRaw>, Option<TypeExpr>) =
            if let Some(raw) = request.entity_type_selector {
                let expanded = expand_type_expression(raw, &ContextualExpander::new(&self.engine, resolver.prepared())).await?;
                let coarse = matches!(expanded, TypeExpr::Type(_)).then(|| expanded.clone()); // clone: the store's specification holds a selector of its own
                let selector = expanded.into_raw()?;
                (selector.named_types(), Some(selector), coarse)
            } else {
                // A write that carries no `type` in its request is matched against registrations by the local entity's actual types -- every one it carries, since clause 5.12 matches on any of them -- so a registration scoped to an unrelated type is not taken to cover the write and refuse it. A write to an entity this broker holds only at a source resolves to an empty set and falls back to id-only matching.
                let types = self.local_entity_types.local_entity_types(target, request.entity_id).await;
                // A stored type name is an IRI; one that were not leaves the
                // superset unnarrowed, which the precise match still decides.
                let coarse = match types.as_slice() {
                    [single] => IriBuf::new(single.to_string()).ok().map(TypeExpr::Type),
                    _ => None,
                };
                (types, None, coarse)
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

        let csrs = self
            .csr_repo
            .find_matching(&[ExpandedEntitySpecification {
                entity_type: coarse_type.as_ref(),
                ids: from_ref(request.entity_id),
                id_pattern: None,
            }])
            .await?;

        let entity = match type_selector.as_ref() {
            Some(selector) => EntitySpecification {
                types: TypeConstraint::Selector(selector),
                ids: from_ref(request.entity_id),
                id_pattern: None,
            },
            None => EntitySpecification::of_entity(&entity_types, request.entity_id),
        };
        let specification = RegistrationMatchSpecification {
            entities: from_ref(&entity),
            attributes: expanded_name.as_slice(),
        };

        // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
        let mut looped_back = Vec::new();
        let mut receiving = Vec::new();
        let mut not_advertising = Vec::new();
        for csr in csrs {
            if csr.access.mode == Some(ForwardingMode::Auxiliary) {
                continue;
            }
            if !specification.covers(&csr) {
                continue;
            }
            if looped {
                // Clause 6.3.17: with the request looped back, a proxied source
                // that holds all the data externally cannot be forwarded to, so
                // a lone one is 508 Loop Detected. Its attributes are still never
                // held here (clause 4.3.6.3). An inclusive registration holds
                // local data, so the loop only stops the forward and the local
                // part still runs.
                if is_proxied(&csr) {
                    looped_back.push(csr);
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
            looped: looped_back,
            receiving,
            not_advertising,
            entity_types,
            expanded_attribute: expanded_name,
        })
    }
}
