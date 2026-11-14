//! Context-source registration discovery policy (ETSI GS CIM 009 clause 5.10.2).

use crate::{
    RegistrationService,
    error::RegistrationError,
    registration_compaction::compacted_registration,
    registration_invariant::RegistrationInvariant,
    registration_validation_reason::RegistrationValidationReason,
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_linked_retrieval::{admission::LinkedOperation, filter_hops::FilterHops};
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
        compilation::{FilterOptIns, compile_q_filter},
        expansion::expand_csf_expr,
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
    /// filter cannot be compiled or the `q` is too complex to resolve (clause
    /// 5.5.6) or names a target language the broker's level does not admit, [`RegistrationError::JsonLd`] when `context`
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
        // RegistrationInfo the query does not match.
        let boundary = scanned
            .first()
            .map(|registration| registration.id.clone())
            .zip(scanned.last().map(|registration| registration.id.clone()));
        let names = NameCompaction::new(self.engine.compactor(context).processed_context().await?);
        // clone: the compiled query keeps the expanded selector, so the serialisable form is taken from a copy.
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

    /// Expand the terms of `params` against `context`. The attributes the `q`
    /// filter and the geoquery name join `attrs`, since clause 5.10.2.4 matches
    /// registrations on them as it does on `attrs`.
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
            // Clause 5.10.2.3 takes neither `jsonKeys` nor `expandValues`, so no
            // attribute is raw JSON and none admits a bare word; and no clause
            // bounds the hops of a discovery query.
            let hops = FilterHops::new(LinkedOperation::QueryRegistrations, None, None);
            let expanded_filter =
                compile_q_filter::<RegistrationError, _, _>(expression, &FilterOptIns::default(), self.filter_admission, &hops, &expander).await?;
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
            Some(expression) => Some(expand_csf_expr(expression, &expander).await?),
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
            .ids(params.id.clone().unwrap_or_default())
            .entity_type(expanded_type)
            .id_pattern(params.id_pattern.clone())
            .attrs(attrs)
            .csf_expr(csf_expr)
            .geo(geo)
            .scope_q(params.scope_q.clone())
            .encountered_sources(encountered_sources.to_vec())
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
/// Clause 5.10.2.5 states the trimming as a recommendation: implementations
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
mod tests {
    use crate::{
        RegistrationService,
        error::RegistrationError,
        fixed_store::{FixedStore, registration},
        service_fixture::{offline_engine, service_over},
        store::RegistrationStore,
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_query::{error::QueryError, page_reach::PageReach, page_window::PageWindow};
    use andromeda_ngsi_ld_request::query::csr::CsrQueryParameters;
    use iri_rs::{Iri, iri};
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

    /// The core context's default vocabulary expands a term no other context
    /// maps.
    const SPEED: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/default-context/speed");
    /// The default vocabulary's `gender`.
    const GENDER: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/default-context/gender");
    /// The default vocabulary's `jp`.
    const JP: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/default-context/jp");

    /// The attributes a discovery query with `q` matches registrations on,
    /// or its refusal.
    async fn q_attributes(q: &str) -> Result<Vec<String>, RegistrationError> {
        let query = serde_urlencoded::to_string([("q", q)]).expect("a query string");
        let params: CsrQueryParameters = serde_urlencoded::from_str(&query).expect("valid discovery");
        let compiled = service(FixedStore::new(Vec::new()))
            .compile_csr_query(&params, &[], &ContextSpec::none())
            .await?;
        Ok(compiled.attrs.iter().map(|iri| iri.as_str().to_owned()).collect())
    }

    /// CP-1, clause 5.10.2.4: the attributes the `q` names join the ones a
    /// registration is matched on, expanded as every pipeline expands them.
    #[tokio::test]
    async fn a_discovery_q_names_its_expanded_attributes() {
        pretty_assertions::assert_eq!(q_attributes("speed>5").await.expect("admitted"), vec![SPEED.as_str()]);
    }

    /// CP-2 and CP-9: clause 5.10.2.3 takes no `expandValues` or `jsonKeys`,
    /// and an operand or a member never names an attribute, so a discovery
    /// query collects the attribute alone.
    #[tokio::test]
    async fn a_discovery_q_collects_the_attribute_whatever_its_operand_or_member() {
        pretty_assertions::assert_eq!(q_attributes("gender==\"Male\"").await.expect("admitted"), vec![GENDER.as_str()]);
        pretty_assertions::assert_eq!(q_attributes("jp[value]==\"x\"").await.expect("admitted"), vec![JP.as_str()]);
    }

    /// CP-3, clause 5.5.6: a discovery `q` past the broker's comparison limit
    /// is too complex to resolve, as in every other request pipeline.
    #[tokio::test]
    async fn a_discovery_q_past_the_comparison_limit_is_too_complex() {
        let values = (0..1001).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
        let refused = q_attributes(&format!("speed=={values}")).await;
        assert!(
            matches!(refused, Err(RegistrationError::Query(QueryError::QTooManyComparisons { .. }))),
            "{refused:?}"
        );
    }

    /// CP-4, clause 4.9: a full language tag is outside the specification's
    /// grammar, so discovery refuses it at that level as every request
    /// pipeline does.
    #[tokio::test]
    async fn a_discovery_q_naming_a_full_tag_is_refused_at_the_specification_level() {
        let refused = q_attributes("name[en-GB]==\"x\"").await;
        assert!(
            matches!(refused, Err(RegistrationError::Query(QueryError::LanguageTagNotAdmitted(_)))),
            "{refused:?}"
        );
    }

    /// Clause 5.10.2.4 bounds none of a discovery query's hops, and discovery
    /// takes no `join`.
    #[tokio::test]
    async fn a_discovery_q_hop_is_admitted() {
        q_attributes("r{a}==1").await.expect("a hop is admitted");
    }
}
