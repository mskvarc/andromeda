//! Observation of inbound HTTP requests: how many were answered, how long they
//! took and how many are in progress.
//!
//! The transport layer hands over the request method, the route the request
//! matched and the status it was answered with; nothing here reads HTTP types.

use crate::names::{HTTP_REQUEST_DURATION_SECONDS, HTTP_REQUESTS_PENDING, HTTP_REQUESTS_TOTAL, LABEL_ENDPOINT, LABEL_METHOD, LABEL_STATUS};
use metrics::{SharedString, counter, gauge, histogram};
use std::{sync::Arc, time::Instant};
use strum::IntoStaticStr;

/// The `method` label of a request, one of the methods RFC 9110 section 9.3
/// defines, or `OTHER` for an extension method so a client cannot mint series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "UPPERCASE")]
pub enum HttpMethod {
    /// `GET`.
    Get,
    /// `HEAD`.
    Head,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `PATCH` (RFC 5789).
    Patch,
    /// `DELETE`.
    Delete,
    /// `OPTIONS`.
    Options,
    /// `CONNECT`.
    Connect,
    /// `TRACE`.
    Trace,
    /// Any extension method.
    Other,
}

/// The `endpoint` label of a request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HttpEndpoint {
    /// The template of the route the request matched, such as
    /// `/ngsi-ld/v1/entities/{entity_id}`; never the raw path.
    Route(Arc<str>),
    /// No route matched. One value for every such request keeps a path scan
    /// from creating one series per path it tries.
    Unmatched,
}

impl HttpEndpoint {
    /// The label value naming this endpoint.
    fn label(&self) -> SharedString {
        match self {
            HttpEndpoint::Route(route) => SharedString::from_shared(Arc::clone(route)),
            HttpEndpoint::Unmatched => SharedString::const_str("unmatched"),
        }
    }
}

/// Marks a response the request counter and duration histogram leave out: the
/// scrape of the metrics themselves, so a scrape never counts itself. Carried
/// in the response's extensions; while in progress, a scrape is pending like
/// any request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmeteredResponse;

/// One request in progress, counted in the pending gauge until it is dropped.
///
/// [`HttpRequestObservation::finish`] records the answered request; dropping
/// the observation unfinished (the client went away, or the request is not
/// counted) only takes it off the pending gauge.
#[derive(Debug)]
pub struct HttpRequestObservation {
    /// The request's method.
    method: HttpMethod,
    /// The route the request matched.
    endpoint: HttpEndpoint,
    /// When the request reached the observing layer.
    started: Instant,
}

impl HttpRequestObservation {
    /// Start observing a request and count it as pending.
    #[must_use]
    pub fn start(method: HttpMethod, endpoint: HttpEndpoint) -> HttpRequestObservation {
        let method_label: &'static str = method.into();
        gauge!(HTTP_REQUESTS_PENDING, LABEL_METHOD => method_label, LABEL_ENDPOINT => endpoint.label()).increment(1.0);
        HttpRequestObservation {
            method,
            endpoint,
            started: Instant::now(),
        }
    }

    /// Record the request as answered with `status`.
    pub fn finish(self, status: u16) {
        let method: &'static str = self.method.into();
        let status = SharedString::from(status.to_string());
        histogram!(
            HTTP_REQUEST_DURATION_SECONDS,
            LABEL_METHOD => method,
            LABEL_ENDPOINT => self.endpoint.label(),
            LABEL_STATUS => status.clone(),
        )
        .record(self.started.elapsed());
        counter!(HTTP_REQUESTS_TOTAL, LABEL_METHOD => method, LABEL_ENDPOINT => self.endpoint.label(), LABEL_STATUS => status).increment(1);
    }
}

impl Drop for HttpRequestObservation {
    fn drop(&mut self) {
        let method: &'static str = self.method.into();
        gauge!(HTTP_REQUESTS_PENDING, LABEL_METHOD => method, LABEL_ENDPOINT => self.endpoint.label()).decrement(1.0);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        http::{HttpEndpoint, HttpMethod, HttpRequestObservation},
        names::{HTTP_REQUEST_DURATION_SECONDS, HTTP_REQUESTS_PENDING, HTTP_REQUESTS_TOTAL},
        recorder::TelemetryRecorder,
    };
    use std::sync::Arc;

    #[test]
    fn an_answered_request_is_counted_under_its_method_route_and_status() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            HttpRequestObservation::start(HttpMethod::Get, HttpEndpoint::Route(Arc::from("/ngsi-ld/v1/entities/{entity_id}"))).finish(200);
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!(
                "{HTTP_REQUESTS_TOTAL}{{method=\"GET\",endpoint=\"/ngsi-ld/v1/entities/{{entity_id}}\",status=\"200\"}} 1"
            )),
            "{rendered}"
        );
        assert!(rendered.contains(&format!("{HTTP_REQUEST_DURATION_SECONDS}_bucket")), "{rendered}");
        Ok(())
    }

    #[test]
    fn a_request_no_route_matched_is_labelled_unmatched() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| HttpRequestObservation::start(HttpMethod::Other, HttpEndpoint::Unmatched).finish(404));
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{HTTP_REQUESTS_TOTAL}{{method=\"OTHER\",endpoint=\"unmatched\",status=\"404\"}} 1")),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn a_request_is_pending_until_its_observation_is_dropped() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        let pending = format!("{HTTP_REQUESTS_PENDING}{{method=\"POST\",endpoint=\"/ngsi-ld/v1/entities\"}}");
        let observation = recorder.record_locally(|| HttpRequestObservation::start(HttpMethod::Post, HttpEndpoint::Route(Arc::from("/ngsi-ld/v1/entities"))));
        let in_progress = recorder.render();
        assert!(in_progress.contains(&format!("{pending} 1")), "{in_progress}");
        recorder.record_locally(|| drop(observation));
        let abandoned = recorder.render();
        assert!(abandoned.contains(&format!("{pending} 0")), "{abandoned}");
        assert!(
            !abandoned.contains(HTTP_REQUESTS_TOTAL),
            "an abandoned request is not counted as answered: {abandoned}"
        );
        Ok(())
    }
}
