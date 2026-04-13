//! Typed `NGSILD-Dataset-Id` request header for Andromeda dataset filtering.
//!
//! Carries one or more dataset identifiers as URNs, equivalent to `datasetId`.

use axum::http::{HeaderName, HeaderValue};
use urn::Urn;

static NGSILD_DATASET_ID: HeaderName = HeaderName::from_static("ngsild-dataset-id");

/// Typed `NGSILD-Dataset-Id` header.
///
/// Values are parsed as a non-empty CSV list of URNs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdDatasetId(Vec<Urn>);

impl NgsiLdDatasetId {
    /// Return dataset identifiers as parsed URNs.
    pub fn as_slice(&self) -> &[Urn] {
        &self.0
    }
}

impl headers::Header for NgsiLdDatasetId {
    fn name() -> &'static HeaderName {
        &NGSILD_DATASET_ID
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdDatasetId, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let mut datasets = Vec::new();
        for value in values {
            let s = match value.to_str() {
                Ok(value) => value,
                Err(_) => return Err(headers::Error::invalid()),
            };
            for segment in s.split(',').map(str::trim) {
                if segment.is_empty() {
                    return Err(headers::Error::invalid());
                }
                let urn = match segment.parse::<Urn>() {
                    Ok(value) => value,
                    Err(_) => return Err(headers::Error::invalid()),
                };
                datasets.push(urn);
            }
        }
        if datasets.is_empty() {
            return Err(headers::Error::invalid());
        }
        Ok(NgsiLdDatasetId(datasets))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let joined = self.0.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
        match HeaderValue::from_str(&joined) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Dataset-Id header"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdDatasetId, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdDatasetId::decode(&mut iter)
    }

    #[test]
    fn decode_single_urn() {
        let header = decode_from("urn:ngsi-ld:Dataset:one").expect("valid");
        pretty_assertions::assert_eq!(header.as_slice().len(), 1);
    }

    #[test]
    fn decode_csv_urns() {
        let header = decode_from("urn:ngsi-ld:Dataset:one, urn:ngsi-ld:Dataset:two").expect("valid");
        pretty_assertions::assert_eq!(header.as_slice().len(), 2);
        pretty_assertions::assert_eq!(header.as_slice()[0].as_str(), "urn:ngsi-ld:Dataset:one");
        pretty_assertions::assert_eq!(header.as_slice()[1].as_str(), "urn:ngsi-ld:Dataset:two");
    }

    #[test]
    fn decode_rejects_non_urn() {
        assert!(decode_from("not-a-urn").is_err());
    }

    #[test]
    fn decode_rejects_empty_segment() {
        assert!(decode_from("urn:ngsi-ld:Dataset:one,").is_err());
    }
}
