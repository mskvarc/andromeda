//! Reading a broker's Prometheus metrics exposition.
//!
//! Several tier assertions are about broker-internal counters -- cold builds,
//! cached-runtime gauges, dormant-tenant wakes -- that are only observable
//! through the metrics endpoint. This scrapes `GET /ngsi-ld/v1/metrics` and
//! parses the named series so a test can read one out.

use reqwest::Client;
use thiserror::Error;

/// Why a metrics scrape could not be read or parsed.
#[derive(Debug, Error)]
pub enum MetricsScrapeError {
    /// The HTTP request to the metrics endpoint failed.
    #[error("metrics request failed: {0}")]
    Request(#[from] reqwest::Error),
    /// The endpoint answered with a non-success status.
    #[error("metrics endpoint returned status {0}")]
    Status(u16),
}

/// One parsed Prometheus sample: a series name, its label set, and its value.
#[derive(Debug, Clone)]
pub struct Sample {
    /// The metric name.
    pub name: String,
    /// The label set as `(key, value)` pairs in exposition order.
    pub labels: Vec<(String, String)>,
    /// The sample value.
    pub value: f64,
}

/// A parsed snapshot of one broker's Prometheus exposition.
#[derive(Debug, Clone)]
pub struct Metrics {
    /// Every parsed sample in the exposition.
    pub samples: Vec<Sample>,
}

impl Metrics {
    /// Scrape and parse the exposition served under `broker_base`.
    ///
    /// `broker_base` is the NGSI-LD API base with a trailing slash, e.g.
    /// `http://127.0.0.1:1067/ngsi-ld/v1/`; the metrics route sits beside the
    /// API families at `metrics`.
    ///
    /// # Errors
    ///
    /// Returns [`MetricsScrapeError::Request`] if the HTTP call fails and
    /// [`MetricsScrapeError::Status`] if the endpoint answers non-2xx.
    pub async fn scrape(client: &Client, broker_base: &str) -> Result<Metrics, MetricsScrapeError> {
        let response = client.get(format!("{broker_base}metrics")).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(MetricsScrapeError::Status(status.as_u16()));
        }
        let body = response.text().await?;
        Ok(Metrics::parse(&body))
    }

    /// Parse a Prometheus text exposition into its samples.
    #[must_use]
    pub fn parse(body: &str) -> Metrics {
        let samples = body.lines().filter_map(parse_line).collect();
        Metrics { samples }
    }

    /// The sum of every sample carrying `name`, or `0.0` when none is present.
    ///
    /// The natural read for a counter split across label sets, e.g. the total
    /// of `andromeda_tenant_cold_builds_total` over its `outcome` labels.
    #[must_use]
    pub fn sum(&self, name: &str) -> f64 {
        self.samples.iter().filter(|sample| sample.name == name).map(|sample| sample.value).sum()
    }

    /// The value of the single sample named `name` carrying exactly `labels`,
    /// if present.
    #[must_use]
    pub fn value(&self, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
        self.samples
            .iter()
            .find(|sample| sample.name == name && labels_match(&sample.labels, labels))
            .map(|sample| sample.value)
    }

    /// The value of the single unlabelled sample named `name`, if present.
    #[must_use]
    pub fn gauge(&self, name: &str) -> Option<f64> {
        self.samples.iter().find(|sample| sample.name == name).map(|sample| sample.value)
    }

    /// Whether any sample carries `name`.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.samples.iter().any(|sample| sample.name == name)
    }
}

/// Whether `actual` carries every `(key, value)` in `expected`.
fn labels_match(actual: &[(String, String)], expected: &[(&str, &str)]) -> bool {
    expected
        .iter()
        .all(|(key, value)| actual.iter().any(|(actual_key, actual_value)| actual_key == key && actual_value == value))
}

/// Parse one exposition line into a [`Sample`], skipping comments, blanks and
/// anything that does not carry a parseable value.
fn parse_line(line: &str) -> Option<Sample> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (identifier, remainder) = if let Some((name, rest)) = line.split_once('{') {
        let (labels_text, after) = rest.split_once('}')?;
        (LineIdentifier { name, labels_text }, after)
    } else {
        let (name, after) = line.split_once(char::is_whitespace)?;
        (LineIdentifier { name, labels_text: "" }, after)
    };
    let value = remainder.split_whitespace().next()?.parse::<f64>().ok()?;
    Some(Sample {
        name: identifier.name.to_owned(),
        labels: parse_labels(identifier.labels_text),
        value,
    })
}

/// The name and raw label text carved off the front of an exposition line.
struct LineIdentifier<'a> {
    /// The metric name.
    name: &'a str,
    /// The text between `{` and `}`, empty when the sample is unlabelled.
    labels_text: &'a str,
}

/// Parse a `key="value",key2="value2"` label list into owned pairs.
fn parse_labels(text: &str) -> Vec<(String, String)> {
    text.split(',')
        .filter(|pair| !pair.trim().is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((key.trim().to_owned(), value.trim().trim_matches('"').to_owned()))
        })
        .collect()
}
