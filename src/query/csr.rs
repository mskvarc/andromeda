use crate::query::{error::QueryError, params::SharedQueryParams};
use serde::Deserialize;
use url::Url;
use urn::Urn;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct CsrQueryParameters {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    pub attrs: Option<String>,
    pub q: Option<String>,
    pub georel: Option<String>,
    pub geometry: Option<String>,
    pub coordinates: Option<String>,
    pub geoproperty: Option<String>,
    pub timerel: Option<String>,
    #[serde(rename = "timeAt")]
    pub time_at: Option<String>,
    #[serde(rename = "endTimeAt")]
    pub end_time_at: Option<String>,
    pub timeproperty: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub options: Option<String>,
}

impl SharedQueryParams for CsrQueryParameters {
    fn raw_id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    fn raw_entity_type(&self) -> Option<&str> {
        self.entity_type.as_deref()
    }
    fn raw_attrs(&self) -> Option<&str> {
        self.attrs.as_deref()
    }
    fn raw_pick(&self) -> Option<&str> {
        None
    }
    fn raw_omit(&self) -> Option<&str> {
        None
    }
    fn raw_dataset_id(&self) -> Option<&str> {
        None
    }
    fn raw_limit(&self) -> Option<u32> {
        self.limit
    }
    fn raw_offset(&self) -> Option<u32> {
        self.offset
    }
}

impl CsrQueryParameters {
    pub fn parse_options(&self) -> CsrQueryParametersOptions {
        let opts = self.options.as_deref().unwrap_or("");
        CsrQueryParametersOptions {
            sys_attrs: opts.contains("sysAttrs"),
        }
    }

    pub fn validate(&self) -> Result<(), QueryError> {
        // At least one filtering parameter is required
        let has_filter =
            self.entity_type.is_some() || self.id.is_some() || self.id_pattern.is_some() || self.attrs.is_some() || self.q.is_some() || self.georel.is_some();
        if !has_filter {
            return Err(QueryError::BadRequestData(
                "At least one of type, id, idPattern, attrs, q, or georel must be provided".to_string(),
            ));
        }

        // Validate each comma-separated id is a valid URI
        if let Some(ref ids) = self.id {
            for id in ids.split(',').map(str::trim) {
                if !id.is_empty() && !is_valid_uri(id) {
                    return Err(QueryError::BadRequestData(format!("Invalid URI in id parameter: {}", id)));
                }
            }
        }

        // Geo query requires georel, geometry, and coordinates together
        if (self.georel.is_some() || self.geometry.is_some() || self.coordinates.is_some())
            && (self.georel.is_none() || self.geometry.is_none() || self.coordinates.is_none())
        {
            return Err(QueryError::BadRequestData(
                "Incomplete geo query: georel, geometry, and coordinates are all required".to_string(),
            ));
        }

        // Temporal query requires timeAt when timerel is present
        if self.timerel.is_some() && self.time_at.is_none() {
            return Err(QueryError::BadRequestData(
                "Incomplete temporal query: timeAt is required when timerel is present".to_string(),
            ));
        }

        // between requires endTimeAt
        if self.timerel.as_deref() == Some("between") && self.end_time_at.is_none() {
            return Err(QueryError::BadRequestData("endTimeAt is required when timerel=between".to_string()));
        }

        Ok(())
    }
}

fn is_valid_uri(id: &str) -> bool {
    if id.starts_with("urn:") {
        return id.parse::<Urn>().is_ok();
    }
    Url::parse(id).is_ok()
}

#[derive(Debug, Default)]
pub struct CsrQueryParametersOptions {
    pub sys_attrs: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params_from_query(query: &str) -> CsrQueryParameters {
        serde_urlencoded::from_str(query).unwrap()
    }

    #[test]
    fn validate_requires_at_least_one_filter() {
        let p = params_from_query("");
        let err = p.validate().expect_err("expected error");
        assert!(err.to_string().contains("At least one of"));
    }

    #[test]
    fn validate_accepts_type_filter() {
        let p = params_from_query("type=Vehicle");
        assert!(p.validate().is_ok());
    }

    #[test]
    fn validate_rejects_invalid_id_uri() {
        let p = params_from_query("id=notavaliduri");
        let err = p.validate().expect_err("expected error");
        assert!(err.to_string().contains("Invalid URI"));
    }

    #[test]
    fn validate_rejects_incomplete_geo() {
        let p = params_from_query("type=Vehicle&georel=within&geometry=Point");
        let err = p.validate().expect_err("expected error");
        assert!(err.to_string().contains("Incomplete geo query"));
    }

    #[test]
    fn validate_rejects_temporal_without_time_at() {
        let p = params_from_query("type=Vehicle&timerel=before");
        let err = p.validate().expect_err("expected error");
        assert!(err.to_string().contains("timeAt is required"));
    }

    #[test]
    fn validate_rejects_between_without_end_time_at() {
        let p = params_from_query("type=Vehicle&timerel=between&timeAt=2024-01-01T00:00:00Z");
        let err = p.validate().expect_err("expected error");
        assert!(err.to_string().contains("endTimeAt is required"));
    }

    #[test]
    fn parse_options_sys_attrs() {
        let p = params_from_query("type=Vehicle&options=sysAttrs");
        assert!(p.parse_options().sys_attrs);
    }

    #[test]
    fn parse_options_default() {
        let p = params_from_query("type=Vehicle");
        assert!(!p.parse_options().sys_attrs);
    }

    #[test]
    fn shared_params_limit_offset() {
        let p = params_from_query("type=Vehicle&limit=50&offset=10");
        assert_eq!(p.effective_limit(), 50);
        assert_eq!(p.effective_offset(), 10);
    }

    #[test]
    fn shared_params_defaults() {
        let p = params_from_query("type=Vehicle");
        assert_eq!(p.effective_limit(), 20);
        assert_eq!(p.effective_offset(), 0);
    }
}
