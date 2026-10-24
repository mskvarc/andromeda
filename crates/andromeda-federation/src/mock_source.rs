//! A mock Context Source for the forwarding tests: responses staged per method
//! and path, served in turn, and every request it received.

use wiremock::{
    Mock,
    MockServer,
    Request,
    ResponseTemplate,
    matchers::{method, path},
};

/// The endpoint a registration names `source` by.
pub(crate) fn endpoint(source: &MockServer) -> String {
    format!("{}/", source.uri())
}

/// Answer `verb served` with `responses` in turn, the last one repeating once
/// the earlier ones are spent.
pub(crate) async fn answer_in_turn(source: &MockServer, verb: &str, served: &str, responses: impl IntoIterator<Item = ResponseTemplate>) {
    let mut responses = responses.into_iter().peekable();
    while let Some(response) = responses.next() {
        let staged = Mock::given(method(verb)).and(path(served)).respond_with(response);
        let staged = if responses.peek().is_some() { staged.up_to_n_times(1) } else { staged };
        staged.mount(source).await;
    }
}

/// Every request `source` received, in arrival order.
pub(crate) async fn received(source: &MockServer) -> Vec<Request> {
    source.received_requests().await.expect("the source records its requests")
}
