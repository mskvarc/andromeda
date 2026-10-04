//! Spelling the name-bearing parameters of a forwarded query string in the
//! forwarding context (clause 4.3.6.6).
//!
//! The parameters that carry names are those the binding tables list
//! ([`crate::forwarded_parameter`]); every other parameter travels as the
//! client wrote it.

use crate::{
    error::FederationError,
    federation_invariant::FederationInvariant,
    forwarded_names::ForwardedNames,
    forwarded_parameter::{ForwardedParameter, ParameterNames},
    forwarding_context::Rewording,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_query::{filter::parser, projection::grammar::parse_projection_list, type_selection::grammar::TypeExprRaw};
use url::form_urlencoded;

impl ForwardedNames<'_> {
    /// A raw query string a client wrote, with every parameter that carries
    /// names (`type`, `attrs`, `jsonKeys`, `expandValues`, `geoproperty`,
    /// `pick`, `omit`, `q` and `csf`) spelled in the forwarding context and every
    /// other parameter unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a name-bearing parameter does
    /// not parse, and the errors of [`ForwardedNames::attr`] and
    /// [`ForwardedNames::q`] and [`ForwardedNames::csf`] when one of its names
    /// does not reword.
    pub(crate) async fn query_string(&self, raw: &str) -> Result<String, FederationError> {
        if self.rewording() == Rewording::AsInbound {
            return Ok(raw.to_owned());
        }
        let pairs = form_urlencoded::parse(raw.as_bytes())
            .into_owned()
            .map(|(key, value)| (key.parse::<ForwardedParameter>().ok(), key, value))
            .collect::<Vec<_>>();
        let mut json_keys = Vec::new();
        for (_, _, value) in pairs.iter().filter(|(parameter, _, _)| *parameter == Some(ForwardedParameter::JsonKeys)) {
            json_keys.extend(attribute_names(value)?);
        }
        // The serializer holds a non-`Sync` encoder, so it is built only once
        // every value is reworded and never lives across an await. A parameter
        // the binding tables do not name carries no name and travels as written.
        let mut reworded = Vec::with_capacity(pairs.len());
        for (parameter, key, value) in &pairs {
            let spelled = match parameter {
                Some(parameter) => self.reworded_parameter(*parameter, value, &json_keys).await?,
                None => value.clone(), // clone: the forwarded query owns the values it writes
            };
            reworded.push((key, spelled));
        }
        Ok(form_urlencoded::Serializer::new(String::new()).extend_pairs(reworded).finish())
    }

    /// One query parameter's value spelled in the forwarding context.
    async fn reworded_parameter(&self, parameter: ForwardedParameter, value: &str, json_keys: &[AttributeName]) -> Result<String, FederationError> {
        match parameter.names() {
            ParameterNames::TypeSelection => {
                let expression: TypeExprRaw = value.parse()?;
                Ok(self.type_expr(&expression).await?.to_string())
            }
            ParameterNames::AttributeList => {
                let mut names = Vec::new();
                for name in attribute_names(value)? {
                    names.push(self.attr(&name).await?.to_string());
                }
                Ok(names.join(","))
            }
            ParameterNames::Attribute => {
                let parsed: NgsiLdReference = value
                    .parse()
                    .map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))?;
                Ok(self.geoproperty(&parsed).await?.to_string())
            }
            ParameterNames::Projection => {
                let terms = parse_projection_list(value)?;
                Ok(self.projection(&terms).await?.iter().map(ToString::to_string).collect::<Vec<_>>().join(","))
            }
            ParameterNames::Query => {
                let expression = parser::parse(value)?;
                Ok(self.q(&expression, Some(json_keys)).await?.to_string())
            }
            ParameterNames::SourceFilter => {
                let expression = parser::parse(value)?;
                Ok(self.csf(&expression).await?.to_string())
            }
            ParameterNames::Nothing => Ok(value.to_owned()),
        }
    }
}

/// The attribute names a comma-separated parameter value lists.
fn attribute_names(value: &str) -> Result<Vec<AttributeName>, FederationError> {
    value
        .split(',')
        .filter(|name| !name.is_empty())
        .map(|name| name.parse().map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error))))
        .collect()
}
