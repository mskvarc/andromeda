//! Request metrics for the API listener: each answered request, its latency
//! and the requests in progress, labelled by the route the request matched.

use andromeda_telemetry::http::{HttpEndpoint, HttpMethod, HttpRequestObservation, UnmeteredResponse};
use axum::{
    extract::{MatchedPath, Request},
    http::Method,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

/// Observe one request: pending while it runs, then counted and timed under
/// the status it was answered with, unless the response is marked
/// [`UnmeteredResponse`].
///
/// Installed with `Router::layer`, so routing has already put the matched
/// route template in the request's extensions; a request no route matched
/// carries none and is labelled `unmatched`.
pub(crate) async fn observe_request(request: Request, next: Next) -> Response {
    let endpoint = request
        .extensions()
        .get::<MatchedPath>()
        .map_or(HttpEndpoint::Unmatched, |route| HttpEndpoint::Route(Arc::from(route.as_str())));
    let observation = HttpRequestObservation::start(http_method(request.method()), endpoint);
    let response = next.run(request).await;
    match response.extensions().get::<UnmeteredResponse>() {
        Some(UnmeteredResponse) => drop(observation),
        None => observation.finish(response.status().as_u16()),
    }
    response
}

/// The `method` label of `method`: one of the methods RFC 9110 section 9.3 and
/// RFC 5789 define, or [`HttpMethod::Other`] for an extension method.
fn http_method(method: &Method) -> HttpMethod {
    match method.as_str() {
        "GET" => HttpMethod::Get,
        "HEAD" => HttpMethod::Head,
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "OPTIONS" => HttpMethod::Options,
        "CONNECT" => HttpMethod::Connect,
        "TRACE" => HttpMethod::Trace,
        _extension => HttpMethod::Other,
    }
}

#[cfg(test)]
mod tests {
    use crate::stack::request_metrics::{http_method, observe_request};
    use andromeda_telemetry::{
        http::{HttpMethod, UnmeteredResponse},
        names::{HTTP_REQUESTS_PENDING, HTTP_REQUESTS_TOTAL},
        recorder::TelemetryRecorder,
    };
    use axum::{
        Extension,
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
        middleware,
        routing::get,
    };
    use std::error::Error;
    use tokio::runtime::Builder;
    use tower::ServiceExt;

    fn router() -> Router {
        Router::new()
            .route("/entities/{entity_id}", get(|| async { StatusCode::OK }))
            .route("/metrics", get(|| async { (Extension(UnmeteredResponse), "scrape") }))
            .layer(middleware::from_fn(observe_request))
    }

    /// Send each of `requests` through [`router`] inside the recorder's local scope.
    fn exchange(recorder: &TelemetryRecorder, requests: Vec<Request<Body>>) -> Result<Vec<StatusCode>, Box<dyn Error>> {
        let runtime = Builder::new_current_thread().build()?;
        recorder.record_locally(|| {
            runtime.block_on(async {
                let mut statuses = Vec::new();
                for request in requests {
                    statuses.push(router().oneshot(request).await?.status());
                }
                Ok::<_, Box<dyn Error>>(statuses)
            })
        })
    }

    #[test]
    fn a_routed_request_is_labelled_with_its_route_template() -> Result<(), Box<dyn Error>> {
        let recorder = TelemetryRecorder::detached()?;
        exchange(&recorder, vec![Request::get("/entities/urn:ngsi-ld:Car:1").body(Body::empty())?])?;
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!(
                "{HTTP_REQUESTS_TOTAL}{{method=\"GET\",endpoint=\"/entities/{{entity_id}}\",status=\"200\"}} 1"
            )),
            "{rendered}"
        );
        assert!(!rendered.contains("urn:ngsi-ld:Car:1"), "{rendered}");
        Ok(())
    }

    #[test]
    fn requests_no_route_matched_share_one_unmatched_series() -> Result<(), Box<dyn Error>> {
        let recorder = TelemetryRecorder::detached()?;
        exchange(
            &recorder,
            vec![Request::get("/wp-admin").body(Body::empty())?, Request::get("/.env").body(Body::empty())?],
        )?;
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{HTTP_REQUESTS_TOTAL}{{method=\"GET\",endpoint=\"unmatched\",status=\"404\"}} 2")),
            "{rendered}"
        );
        assert!(!rendered.contains("wp-admin"), "{rendered}");
        Ok(())
    }

    #[test]
    fn an_unmetered_response_is_not_counted_and_leaves_nothing_pending() -> Result<(), Box<dyn Error>> {
        let recorder = TelemetryRecorder::detached()?;
        let statuses = exchange(&recorder, vec![Request::get("/metrics").body(Body::empty())?])?;
        pretty_assertions::assert_eq!(statuses, [StatusCode::OK]);
        let rendered = recorder.render();
        assert!(!rendered.contains(&format!("{HTTP_REQUESTS_TOTAL}{{")), "{rendered}");
        assert!(
            rendered.contains(&format!("{HTTP_REQUESTS_PENDING}{{method=\"GET\",endpoint=\"/metrics\"}} 0")),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn an_extension_method_is_labelled_other() -> Result<(), Box<dyn Error>> {
        pretty_assertions::assert_eq!(http_method(&Method::from_bytes(b"PROPFIND")?), HttpMethod::Other);
        pretty_assertions::assert_eq!(http_method(&Method::PATCH), HttpMethod::Patch);
        Ok(())
    }
}
