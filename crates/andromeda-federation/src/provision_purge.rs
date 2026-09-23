//! Federated Purge Entities (clause 5.6.21.4, bound to `DELETE /entities`
//! by clause 6.4.3.3).
//!
//! Unless the request is local, the purge is forwarded to every matching
//! registration that advertises `purgeEntity`; a proxied registration that
//! matches without advertising it fails with a Conflict, as it would for any
//! other write it cannot perform (clause 5.6.21.4).

use crate::{
    FederationService,
    error::FederationError,
    forwarding_url::forwarding_url,
    operation::ForwardingOperation,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    provision::{PlannedWrite, is_proxied, proxied_mode},
    registration_selection::information_matches_entity,
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use tracing::debug;
use url::form_urlencoded;

/// Parameters for forwarding a purge.
pub struct ForwardPurgeRequest<'a> {
    /// The query's type selector, used for registration preselection.
    pub entity_type_compact: Option<&'a TypeExprRaw>,
    /// The query's identifiers, used for registration preselection.
    pub entity_ids: Option<&'a [Uri]>,
    /// The identifiers the query selected locally; a registration's failure is
    /// reported against the ones it covers.
    pub local_ids: &'a [Uri],
    /// The client's query string; its `local` parameter is not forwarded
    /// (clause 6.3.18 binds it to this broker's own forwarding decision).
    pub query_string: &'a str,
    /// The Entity Map the purge runs under (clause 5.5.14): when a map is
    /// referenced, the fan-out is fixed to its registrations and each forwarded
    /// DELETE carries the map that source is linked to, exactly as a forwarded
    /// query or retrieval does.
    pub entity_map: EntityMapInUse<'a>,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
}

/// What one registration answered to a forwarded purge.
pub struct ForwardedPurgeResult {
    /// The forwarding attempt.
    pub result: ForwardedWriteResult,
    /// The entities a failure of this registration is reported against
    /// (Table 5.2.17-1 needs an `entityId` per error): the locally selected
    /// identifiers the registration covers, else the identifiers its
    /// information names, else the registration itself.
    pub reported_ids: Vec<Uri>,
}

/// The client's query without the `local` parameter.
fn forwarded_query_string(query_string: &str) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(form_urlencoded::parse(query_string.as_bytes()).filter(|(name, _)| name != "local"));
    serializer.finish()
}

/// The client's query with `local` and its own `id` dropped and the map-narrowed
/// identifiers put in their place (clause 5.5.14).
///
/// Under a referenced Entity Map the local phase was held to the intersection of
/// the client's identifiers and the map's (clause 5.5.14), so the forwarded
/// DELETE must carry that same narrowed set rather than the client's verbatim
/// list; the other filters (`type`, `q`, `attrs`, `drop`/`keep`) travel
/// unchanged so each source purges exactly what the map fixed.
fn narrowed_query_string(query_string: &str, narrowed_ids: Option<&[Uri]>) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(form_urlencoded::parse(query_string.as_bytes()).filter(|(name, _)| name != "local" && name != "id"));
    if let Some(ids) = narrowed_ids.filter(|ids| !ids.is_empty()) {
        serializer.append_pair("id", &ids.iter().map(Uri::as_str).collect::<Vec<_>>().join(","));
    }
    serializer.finish()
}

/// The identifiers a registration's purge failure is reported against.
fn reported_ids(csr: &ContextSourceRegistration, local_ids: &[Uri]) -> Vec<Uri> {
    let covered = local_ids
        .iter()
        .filter(|id| {
            csr.coverage
                .information
                .iter()
                .any(|information| information_matches_entity(information, &[], Some(id)))
        })
        .cloned() // clone: the report owns the identifiers it names
        .collect::<Vec<_>>();
    if !covered.is_empty() {
        return covered;
    }
    let declared = csr
        .coverage
        .information
        .iter()
        .flat_map(|information| information.entities.iter().flatten())
        .filter_map(|selector| selector.id.clone()) // clone: same
        .collect::<Vec<_>>();
    if !declared.is_empty() {
        return declared;
    }
    vec![csr.id.clone()] // clone: same
}

impl FederationService {
    /// Forward a purge to every matching registration.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type
    /// cannot be expanded against the request's context, and
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read. A registration that refuses the purge does not fail
    /// the call: its refusal is reported as a [`ForwardedPurgeResult`].
    pub async fn forward_purge(&self, target: &ContextTarget, request: ForwardPurgeRequest<'_>) -> Result<Vec<ForwardedPurgeResult>, FederationError> {
        if !target.permits_federation() {
            return Ok(Vec::new());
        }
        if self.is_loop_detected(request.triggering.via) {
            debug!("Loop detected in Via header chain, skipping purge forwarding");
            return Ok(Vec::new());
        }
        let prepared_context = self.engine.prepare_term_context(request.context);
        let expanded_type = match request.entity_type_compact.and_then(TypeExprRaw::as_single_type) {
            Some(name) => Some(self.engine.expand_type(name.as_str(), &prepared_context).await?),
            None => None,
        };
        let first_id = request.entity_ids.and_then(|ids| ids.first());
        // Clause 5.5.14 (and clause 5.12, last paragraph): a referenced map fixes
        // the sources this purge contacts to the map's own registrations, in
        // place of clause 5.12 matching, exactly as a forwarded query does.
        let csrs = match request.entity_map.referenced() {
            Some(map) => self.registrations_of_map(map).await?,
            None => self.csr_repo.find_matching(expanded_type.as_ref(), first_id).await?,
        };
        let query_string = match request.entity_map.referenced() {
            Some(_) => narrowed_query_string(request.query_string, request.entity_ids),
            None => forwarded_query_string(request.query_string),
        };

        // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
        let mut results = Vec::new();
        let mut planned = Vec::new();
        let mut reported = Vec::new();
        for csr in csrs.into_iter().filter(|csr| csr.access.mode != Some(ForwardingMode::Auxiliary)) {
            let ids = reported_ids(&csr, request.local_ids);
            if ForwardingOperation::PurgeEntity.matches(&csr) {
                let url = forwarding_url(csr.access.endpoint.url(), &["entities"], &query_string);
                // Clause 5.5.14: the source is told the map it is linked to under
                // the map in use, so it joins the same unitary sequence the query
                // that built the map did.
                let linked_map = request.entity_map.location_for(&csr.id).cloned(); // clone: the planned write owns the map location it carries
                reported.push((csr.id.clone(), ids)); // clone: the report is keyed by the registration the write contacts
                planned.push(PlannedWrite {
                    registration: csr,
                    url,
                    method: reqwest::Method::DELETE,
                    body: None,
                    proxied_attrs: Vec::new(),
                    context: request.context,
                    triggering: request.triggering,
                    linked_map,
                });
            } else if is_proxied(&csr) {
                let mode = proxied_mode(&csr);
                results.push(ForwardedPurgeResult {
                    result: ForwardedWriteResult {
                        registration_id: csr.id,
                        mode,
                        outcome: ForwardedWriteOutcome::NotAdvertised,
                        proxied_attrs: Vec::new(),
                    },
                    reported_ids: ids,
                });
            }
        }
        for result in self.execute_writes(planned).await {
            let reported_ids = reported
                .iter()
                .find(|(registration, _)| *registration == result.registration_id)
                .map(|(_, ids)| ids.clone()) // clone: the result owns the identifiers it reports against
                .unwrap_or_default();
            results.push(ForwardedPurgeResult { result, reported_ids });
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use crate::provision_purge::{forwarded_query_string, narrowed_query_string, reported_ids};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use serde_json::{Value, json};

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("valid URI")
    }

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "mode": "redirect",
            "information": [information],
            "operations": ["purgeEntity"],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    #[test]
    fn the_local_parameter_is_not_forwarded_and_the_rest_of_the_query_is() {
        pretty_assertions::assert_eq!(forwarded_query_string("type=Vehicle&local=false&q=speed%3E5"), "type=Vehicle&q=speed%3E5");
        pretty_assertions::assert_eq!(forwarded_query_string("local=true"), "");
    }

    /// Clause 5.5.14: under a referenced map the forwarded DELETE carries the
    /// map-narrowed identifiers, not the client's verbatim `id`, while the other
    /// filters travel unchanged.
    #[test]
    fn the_narrowed_identifiers_replace_the_clients_own() {
        let narrowed = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        pretty_assertions::assert_eq!(
            narrowed_query_string("type=Vehicle&id=urn:ngsi-ld:Vehicle:9&local=false&q=speed%3E5", Some(&narrowed)),
            "type=Vehicle&q=speed%3E5&id=urn%3Angsi-ld%3AVehicle%3A1%2Curn%3Angsi-ld%3AVehicle%3A2"
        );
        // With no narrowed identifiers the client's `id` is still dropped: the
        // map fixed the set and the peer scopes by the map it is told.
        pretty_assertions::assert_eq!(narrowed_query_string("type=Vehicle&id=urn:ngsi-ld:Vehicle:9", None), "type=Vehicle");
    }

    #[test]
    fn a_failure_is_reported_against_the_covered_local_ids_then_the_declared_ids_then_the_registration() {
        let by_id = registration(&json!({"entities": [{"id": "urn:ngsi-ld:Vehicle:1", "type": "https://example.org/Vehicle"}]}));
        let local = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        pretty_assertions::assert_eq!(reported_ids(&by_id, &local), vec![uri("urn:ngsi-ld:Vehicle:1")]);
        pretty_assertions::assert_eq!(reported_ids(&by_id, &[]), vec![uri("urn:ngsi-ld:Vehicle:1")]);
        let by_type = registration(&json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        pretty_assertions::assert_eq!(reported_ids(&by_type, &[]), vec![uri("urn:ngsi-ld:csr:1")]);
        pretty_assertions::assert_eq!(reported_ids(&by_type, &local), local.to_vec());
    }
}
