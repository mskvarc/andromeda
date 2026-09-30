//! Federated type and attribute discovery queries (clauses 5.7.5 to 5.7.10, 6.3.17).

use crate::{
    FederationService,
    answer_vocabulary::AnswerVocabulary,
    cache::Cacheability,
    entity_read::own_loop_warning,
    error::FederationError,
    forwarded_names::ForwardedNames,
    forwarding_context::BodyPresence,
    forwarding_url::build_discovery_url,
    outcome::{ForwardGetOutcome, ForwardedDiscoveryResult},
    registration_payload::forwarding_decision,
    request::{DiscoveryTarget, ForwardDiscoveryRequest},
    source_read::SourceRead,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use futures::{StreamExt, stream};
use tracing::warn;

/// How many active registrations one discovery pagination round-trip loads.
const ACTIVE_REGISTRATION_PAGE: u32 = 500;

impl FederationService {
    /// Forward a type or attribute discovery query to every active registration
    /// that advertises the operation.
    pub async fn forward_discovery_query(&self, target: &ContextTarget, request: ForwardDiscoveryRequest<'_>) -> Vec<ForwardedDiscoveryResult> {
        if !target.permits_federation() {
            return Vec::new();
        }
        let ForwardDiscoveryRequest {
            target: discovered,
            query_string,
            context,
            operation,
            triggering,
        } = request;
        if self.is_loop_detected(triggering.via) {
            return vec![ForwardedDiscoveryResult {
                body: None,
                warnings: vec![own_loop_warning()],
            }];
        }

        // Keyset-paginate through all active CSRs; discovery forwarding must
        // consider every registration, but the repository caps each round-trip
        // so a runaway CSR table cannot blow up memory.
        let page_capacity = match usize::try_from(ACTIVE_REGISTRATION_PAGE) {
            Ok(value) => value,
            Err(error) => {
                warn!(%error, "discovery forwarding page size does not fit this platform");
                return Vec::new();
            }
        };
        let mut csrs = Vec::new();
        let mut cursor: Option<Identifier> = None;
        loop {
            let page = match self.csr_repo.find_all_active(cursor.as_ref(), ACTIVE_REGISTRATION_PAGE).await {
                Ok(page) => page,
                Err(e) => {
                    warn!("Failed to load active CSRs for discovery forwarding: {}", e);
                    return Vec::new();
                }
            };
            if page.is_empty() {
                break;
            }
            let page_len = page.len();
            cursor = page.last().map(|registration| registration.id.clone()); // clone: the cursor must outlive the page moved below.
            csrs.extend(page.into_iter().filter(|registration| operation.matches(registration)));
            if page_len < page_capacity {
                break;
            }
        }

        if csrs.is_empty() {
            return Vec::new();
        }

        let resolver = TermResolver::new(&self.engine, context);
        let resolver = &resolver;
        stream::iter(csrs)
            .map(|csr| async move {
                let url = match self.spelled_discovery(&csr, context, resolver, discovered, query_string).await {
                    Ok((segments, query_string)) => {
                        let segments = segments.iter().map(String::as_str).collect::<Vec<_>>();
                        build_discovery_url(csr.access.endpoint.url(), &segments, &query_string)
                    }
                    Err(error) => Err(error),
                };
                let read = SourceRead::builder()
                    .registration(&csr)
                    .inbound(context)
                    .triggering(triggering)
                    .vocabulary(AnswerVocabulary::Discovery)
                    .build();
                // Clause 6.3.17 allows caching for `/entities` only.
                let outcome = self.forward_get(read, url, Cacheability::Uncacheable).await;
                match outcome {
                    ForwardGetOutcome::Success(response) => Some(ForwardedDiscoveryResult {
                        body: Some(response.body),
                        warnings: response.warnings,
                    }),
                    // 404 is not abnormal per spec clause 6.3.17
                    ForwardGetOutcome::NotFound(_) => None,
                    other @ (ForwardGetOutcome::InvalidPayload(_)
                    | ForwardGetOutcome::UnreadableNames(_)
                    | ForwardGetOutcome::ErrorStatus(_)
                    | ForwardGetOutcome::Transport(_)
                    | ForwardGetOutcome::LoopDetected) => {
                        let warning = other.warning(&csr.id)?;
                        warn!("Discovery forwarding to context source {} failed: {}", csr.id, warning);
                        Some(ForwardedDiscoveryResult {
                            body: None,
                            warnings: vec![warning],
                        })
                    }
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .filter_map(|entry| async move { entry })
            .collect()
            .await
    }

    /// The discovery path segments and query string `registration` is sent,
    /// with the type or attribute a path names and every name-bearing parameter
    /// spelled in the registration's forwarding context (clause 4.3.6.6).
    ///
    /// # Errors
    ///
    /// Returns the failure to decide the registration's forwarding context, or
    /// to spell a name in it.
    async fn spelled_discovery(
        &self,
        registration: &ContextSourceRegistration,
        inbound: &ContextSpec,
        resolver: &TermResolver<'_>,
        target: DiscoveryTarget<'_>,
        query_string: &str,
    ) -> Result<(Vec<String>, String), FederationError> {
        let decision = forwarding_decision(registration, inbound, BodyPresence::Absent)?;
        let naming = decision.context.naming(inbound);
        let names = ForwardedNames::new(&self.engine, resolver, &naming, decision.context.rewording());
        let collection = collection_of(target).to_owned();
        let segments = match target {
            DiscoveryTarget::Types | DiscoveryTarget::Attributes => vec![collection],
            DiscoveryTarget::Type(name) => vec![collection, names.entity_type(name).await?.to_string()],
            DiscoveryTarget::Attribute(name) => vec![collection, names.attr(name).await?.to_string()],
        };
        Ok((segments, names.query_string(query_string).await?))
    }
}

/// The collection segment of type discovery (clauses 5.7.5 to 5.7.7).
const TYPES_COLLECTION: &str = "types";
/// The collection segment of attribute discovery (clauses 5.7.8 to 5.7.10).
const ATTRIBUTES_COLLECTION: &str = "attributes";

/// The collection a discovery target sits in.
const fn collection_of(target: DiscoveryTarget<'_>) -> &'static str {
    match target {
        DiscoveryTarget::Types | DiscoveryTarget::Type(_) => TYPES_COLLECTION,
        DiscoveryTarget::Attributes | DiscoveryTarget::Attribute(_) => ATTRIBUTES_COLLECTION,
    }
}

#[cfg(test)]
mod tests {
    use crate::{discovery::collection_of, request::DiscoveryTarget};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

    #[test]
    fn every_discovery_target_sits_in_its_collection() {
        let vehicle: EntityTypeName = "Vehicle".parse().expect("a type name");
        let speed: AttributeName = "https://example.org/ns/speed".parse().expect("an attribute name");
        pretty_assertions::assert_eq!(collection_of(DiscoveryTarget::Types), "types");
        pretty_assertions::assert_eq!(collection_of(DiscoveryTarget::Type(&vehicle)), "types");
        pretty_assertions::assert_eq!(collection_of(DiscoveryTarget::Attributes), "attributes");
        pretty_assertions::assert_eq!(collection_of(DiscoveryTarget::Attribute(&speed)), "attributes");
    }
}
