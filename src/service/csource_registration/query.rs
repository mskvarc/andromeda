//! Context-source registration discovery queries.

use crate::{error::AndromedaError, service::csource_registration::CsourceRegistrationService};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_query::{
    csr::compiled::{CompiledCsrQuery, CompiledCsrTemporalFilter},
    geo::compile as geo_compile,
    pagination::{CountPolicy, Pagination},
    projection::expansion::expand_attrs,
    temporal::window::TimeInterval,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, csr::CsrQueryParameters};
use serde_json::Value;

impl CsourceRegistrationService {
    /// Query registrations matching the supplied discovery parameters.
    pub async fn query(&self, params: &CsrQueryParameters, context: &ContextSpec) -> Result<(Vec<Value>, i64), AndromedaError> {
        params.validate().map_err(|error| AndromedaError::csr_validation(error.to_string()))?;
        let compiled = self.compile_csr_query(params, context).await?;
        let (registrations, total) = self.repo.query_compiled(&compiled).await?;
        let mut values = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let mut value = serde_json::to_value(registration).map_err(|error| AndromedaError::internal(format!("Failed to serialize CSR: {error}")))?;
            if let Some(information) = value.get_mut("information") {
                self.compact_information(information, context).await?;
            }
            values.push(value);
        }
        Ok((values, total))
    }

    async fn compile_csr_query(&self, params: &CsrQueryParameters, context: &ContextSpec) -> Result<CompiledCsrQuery, AndromedaError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared_context);
        let expanded_type = match params.entity_type.as_ref() {
            Some(selection) => match selection.as_single_type() {
                Some(entity_type) => Some(expander.expand_type(entity_type.as_str()).await?),
                None => return Err(AndromedaError::csr_validation("CSR discovery requires a single entity type")),
            },
            None => None,
        };
        let expanded_attrs = expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?;
        let geo = geo_compile::compile_geo_query(params.geo.0.as_ref(), &expander).await?;
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
            .attrs(expanded_attrs)
            .q_expr(params.q.clone()) // clone: the compiled query owns the parsed expression.
            .geo(geo)
            .temporal(temporal)
            .pagination(Pagination {
                limit: params.effective_limit(),
                offset: params.effective_offset(),
                count: CountPolicy::Omit,
            })
            .build())
    }
}
