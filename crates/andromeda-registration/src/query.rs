//! Context-source registration discovery policy (ETSI GS CIM 009 clause 5.10.2).

use crate::{
    RegistrationService,
    error::RegistrationError,
    registration_compaction::compacted_registration,
    registration_invariant::RegistrationInvariant,
    registration_validation_reason::RegistrationValidationReason,
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    registration::ContextSourceRegistration,
    registration_matching::{
        entity_specification::{EntitySpecification, TypeConstraint},
        match_specification::RegistrationMatchSpecification,
    },
};
use andromeda_ngsi_ld_query::{
    csr::compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter},
    filter::{
        expansion::{apply_expand_values, expand_csf_expr, expand_q_expr},
        json_key_attributes::JsonKeyAttributes,
        path::expanded_attribute_name,
        referenced_attributes::referenced_attributes,
    },
    geo::compile as geo_compile,
    page_reach::PageReach,
    page_window::PageWindow,
    projection::expansion::expand_attrs,
    temporal::window::TimeInterval,
    type_selection::{
        expansion::expand_type_expression,
        grammar::{TypeExpr, TypeExprRaw},
    },
};
use andromeda_ngsi_ld_request::query::csr::CsrQueryParameters;
use iri_rs::IriBuf;
use std::collections::HashSet;

/// The registrations a discovery query answers with, and the pagination it
/// reports around them (clause 5.10.2).
pub struct DiscoveredRegistrations {
    /// The clause-5.10.2.5-trimmed, response-ready registrations, their names
    /// compacted under the request's context.
    pub registrations: Vec<ContextSourceRegistration>,
    /// Total matches before pagination (clause 6.3.13).
    pub total: u64,
    /// The first and last registration identifiers of the raw store page,
    /// ascending, from which a clause 5.5.9.1 cursor pointer is computed. Read
    /// from the untrimmed page so the walk continues past every registration the
    /// store scanned, not only the ones the clause-5.10.2.5 trim kept. Absent on
    /// an empty page.
    pub boundary: Option<(Identifier, Identifier)>,
    /// Whether the collection continues past the raw store page in the
    /// direction it was read, so a page the clause-5.10.2.5 trim thinned still
    /// continues (clause 5.5.9.1).
    pub reach: PageReach,
}

impl RegistrationService {
    /// Discover the page `window` selects of the registrations matching
    /// `params` (clause 5.10.2), excluding the sources the request already
    /// passed through (clause 5.12).
    ///
    /// Each returned registration matches on at least one of its
    /// `RegistrationInfo` elements (clause 5.12) and is compacted under
    /// `context`.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::Validation`] when `params` is not a
    /// well-formed discovery request or names an attribute that is not an IRI once expanded,
    /// [`RegistrationError::Query`] when the `q`, `csf`, geo or temporal
    /// filter cannot be compiled, [`RegistrationError::JsonLd`] when `context`
    /// cannot be loaded or a response cannot be compacted under it, and
    /// [`RegistrationError::Persistence`] when the store rejects the query.
    pub async fn query(
        &self,
        params: &CsrQueryParameters,
        window: &PageWindow,
        encountered_sources: &[ContextSourcePseudonym],
        context: &ContextSpec,
    ) -> Result<DiscoveredRegistrations, RegistrationError> {
        params
            .validate()
            .map_err(|error| RegistrationError::Validation(RegistrationValidationReason::QueryParameters(error)))?;
        let compiled = self.compile_csr_query(params, encountered_sources, context).await?;
        let page = self.repo.query_compiled(&compiled, &window.with_lookahead()).await?;
        let total = page.total;
        let (scanned, reach) = window.settle(page.registrations);
        // Clause 5.5.9.1: the cursor pointers are read from the raw store page,
        // before the clause-5.10.2.5 trim below drops the registrations whose
        // RegistrationInfo the query does not match. clone: the boundary
        // outlives the page the loop consumes.
        let boundary = scanned
            .first()
            .map(|registration| registration.id.clone())
            .zip(scanned.last().map(|registration| registration.id.clone()));
        let names = NameCompaction::new(self.engine.compactor(context).processed_context().await?);
        // clone: clause 5.12 matching reads the selector in its serialisable
        // shape, and the compiled query keeps the expanded one.
        let type_selector = compiled
            .entity_type
            .clone()
            .map(TypeExpr::into_raw)
            .transpose()
            .map_err(RegistrationInvariant::ExpandedTypeName)?;
        let mut matched = Vec::with_capacity(scanned.len());
        for registration in scanned {
            if let Some(registration) = registration_matching_the_query(registration, &compiled, type_selector.as_ref())? {
                matched.push(compacted_registration(&names, registration));
            }
        }
        Ok(DiscoveredRegistrations {
            registrations: matched,
            total,
            boundary,
            reach,
        })
    }

    async fn compile_csr_query(
        &self,
        params: &CsrQueryParameters,
        encountered_sources: &[ContextSourcePseudonym],
        context: &ContextSpec,
    ) -> Result<CompiledCsrQuery, RegistrationError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared_context);
        let expanded_type = match params.entity_type.as_ref() {
            Some(selection) => Some(expand_type_expression(selection, &expander).await?),
            None => None,
        };
        let mut attrs = expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?;
        // Clause 5.5.6: a reference geometry past the budget makes the
        // discovery query too complex to resolve.
        let geo = geo_compile::compile_geo_query(params.geo.0.as_ref(), &expander, self.geo_admission).await?;
        // Clause 5.10.2.4: the attributes the `q` filter and the geoquery name
        // match registrations exactly as the `attrs` list does, so they join
        // it; the filter itself is not evaluated against a registration.
        if let Some(expression) = params.q.clone() {
            // clone: expansion owns and rewrites the request expression without mutating cached parameters.
            let expanded_filter = expand_q_expr(expression, &JsonKeyAttributes::default(), &expander).await?;
            // Discovery takes no `expandValues`, so no attribute admits a bare word.
            let expanded_filter = apply_expand_values(expanded_filter, &HashSet::<AttributeName>::new(), &expander).await?;
            for name in referenced_attributes(&expanded_filter) {
                let iri = IriBuf::new(name.as_str().to_owned())
                    .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::QNonIriAttribute { source }))?;
                if !attrs.contains(&iri) {
                    attrs.push(iri);
                }
            }
        }
        if let Some(geo) = &geo {
            let iri = IriBuf::new(geo.geoproperty.as_str().to_owned())
                .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::GeopropertyNotIri { source }))?;
            if !attrs.contains(&iri) {
                attrs.push(iri);
            }
        }
        let csf_expr = match params.csf.clone() {
            Some(expression) => Some(expand_csf_expr(expression, &expander).await?), // clone: same reason as `q`.
            None => None,
        };
        let temporal = if let (Some(timerel), Some(time_at)) = (&params.timerel, &params.time_at) {
            Some(
                CompiledCsrTemporalFilter::builder()
                    .interval(TimeInterval::from_parts(Some(*timerel), Some(*time_at), params.end_time_at)?)
                    .timeproperty(params.timeproperty)
                    .build(),
            )
        } else {
            None
        };

        Ok(CompiledCsrQuery::builder()
            .ids(params.id.clone().unwrap_or_default()) // clone: the compiled query owns request identifiers across the store await.
            .entity_type(expanded_type)
            .id_pattern(params.id_pattern.clone()) // clone: the compiled query owns the validated pattern.
            .attrs(attrs)
            .csf_expr(csf_expr)
            .geo(geo)
            .scope_q(params.scope_q.clone()) // clone: the compiled query owns the parsed scope query.
            .encountered_sources(encountered_sources.to_vec()) // clone: the compiled query owns the Via listing across the store await.
            .temporal(temporal)
            .build())
    }
}

/// The registration trimmed to the `RegistrationInfo` elements the compiled
/// query matches (clause 5.10.2.5), or nothing when none match.
///
/// The statement preselects on the clause 5.12 reading, evaluating the two
/// regular-expression cases on the portable restatements of the patterns, which
/// agree with the in-memory engine; the typed evaluation here is the authority,
/// and it is what settles the response.
///
/// Clause 5.10.2.5 states the trimming as a recommendation -- implementations
/// "should return filtered Context Source Registrations, which only contain
/// context source registration information relevant for the query, in
/// particular only matching `RegistrationInfo` elements". Andromeda adopts it as
/// behaviour: it answers what a discovery client actually asks (which parts of
/// a source are relevant to the query), and it keeps parity with the Context
/// Source Notification path, which trims on the identically worded clause
/// 5.11.7 recommendation. A registration none of whose elements match is
/// dropped entirely.
fn registration_matching_the_query(
    registration: ContextSourceRegistration,
    compiled: &CompiledCsrQuery,
    type_selector: Option<&TypeExprRaw>,
) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
    let entity = EntitySpecification {
        types: type_selector.map_or(TypeConstraint::Any, TypeConstraint::Selector),
        ids: &compiled.ids,
        id_pattern: compiled.id_pattern.as_ref(),
    };
    let entities = if compiled.entity_type.is_some() || !compiled.ids.is_empty() || compiled.id_pattern.is_some() {
        vec![entity]
    } else {
        Vec::new()
    };
    let attributes = compiled.attrs.iter().map(expanded_attribute_name).collect::<Result<Vec<AttributeName>, _>>()?;
    let specification = RegistrationMatchSpecification {
        entities: &entities,
        attributes: &attributes,
    };
    // Clause 5.10.2.5: keep only the RegistrationInfo elements the query
    // matches, and drop the whole registration when none remain.
    let mut registration = registration;
    registration.coverage.information.retain(|info| specification.matches_information(info));
    Ok((!registration.coverage.information.is_empty()).then_some(registration))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        RegistrationService,
        fixed_store::{FixedStore, registration},
        service_fixture::{offline_engine, service_over},
        store::RegistrationStore,
    };
    use andromeda_jsonld::context_spec::ContextSpec;

    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;

    use andromeda_ngsi_ld_query::{page_reach::PageReach, page_window::PageWindow};
    use andromeda_ngsi_ld_request::query::csr::CsrQueryParameters;
    use serde_json::json;
    use std::sync::Arc;

    fn service(store: FixedStore) -> RegistrationService {
        service_over(Arc::new(store) as Arc<dyn RegistrationStore>, offline_engine(), None)
    }

    fn of_type(id: &str, entity_type: &str) -> ContextSourceRegistration {
        registration(id, "inclusive", &json!([{"entities": [{"type": entity_type}]}]))
    }

    /// Clause 5.10.2.5 trims the registrations whose information the query
    /// does not match after the store paged them, so the cursor boundary and
    /// whether the collection continues are read from the raw page: a walk
    /// continues past every registration the store scanned.
    #[tokio::test]
    async fn the_cursor_boundary_and_reach_come_from_the_raw_page() {
        let store = FixedStore::new(vec![
            of_type("urn:ngsi-ld:ContextSourceRegistration:1", "https://example.org/Building"),
            of_type("urn:ngsi-ld:ContextSourceRegistration:2", "https://example.org/Vehicle"),
            of_type("urn:ngsi-ld:ContextSourceRegistration:3", "https://example.org/Building"),
            of_type("urn:ngsi-ld:ContextSourceRegistration:4", "https://example.org/Building"),
        ]);
        let params: CsrQueryParameters = serde_urlencoded::from_str("type=https%3A%2F%2Fexample.org%2FVehicle&limit=3").expect("valid discovery");
        let window = PageWindow::offset(3, 0);

        let discovered = service(store).query(&params, &window, &[], &ContextSpec::none()).await.expect("discovered");

        pretty_assertions::assert_eq!(discovered.registrations.len(), 1, "the trim keeps the one matching registration");
        pretty_assertions::assert_eq!(discovered.reach, PageReach::Continues);
        pretty_assertions::assert_eq!(
            discovered.boundary,
            Some((
                "urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("valid URI"),
                "urn:ngsi-ld:ContextSourceRegistration:3".parse().expect("valid URI")
            ))
        );
    }
}
