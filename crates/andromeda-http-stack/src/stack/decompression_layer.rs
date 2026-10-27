//! Mapping of the decompression configuration onto a `tower-http` layer, and
//! the record of the `Content-Length` that decoding removes.

use andromeda_configuration::schema::{
    content_coding::ContentCoding,
    http::decompression::{DecompressionConfig, UnacceptedEncoding},
};
use andromeda_http_extraction::content_length_precondition::ReceivedContentLength;
use axum::extract::Request;
use tower_http::decompression::RequestDecompressionLayer;

/// Build the request-decompression layer, or `None` when no coding is accepted.
///
/// The layer starts with every coding on and each absent coding is switched off,
/// so the accepted set is exactly `config.algorithms`. An empty set means no
/// inbound decompression at all, and no layer is installed.
pub(crate) fn build_decompression_layer(config: &DecompressionConfig) -> Option<RequestDecompressionLayer> {
    if config.algorithms.is_empty() {
        return None;
    }
    let mut layer = RequestDecompressionLayer::new();
    if !config.algorithms.contains(ContentCoding::Gzip) {
        layer = layer.no_gzip();
    }
    if !config.algorithms.contains(ContentCoding::Brotli) {
        layer = layer.no_br();
    }
    if !config.algorithms.contains(ContentCoding::Deflate) {
        layer = layer.no_deflate();
    }
    if !config.algorithms.contains(ContentCoding::Zstd) {
        layer = layer.no_zstd();
    }
    Some(layer.pass_through_unaccepted(matches!(config.unaccepted_encoding, UnacceptedEncoding::PassThrough)))
}

/// Record the `Content-Length` a request was received with, ahead of the
/// decompression layer that removes it from a decoded request.
///
/// ETSI GS CIM 009 v1.9.1 clause 6.3.4 checks the length the client declared,
/// which only this record keeps once the body is decoded.
pub(crate) async fn record_received_content_length(mut request: Request) -> Request {
    let received = ReceivedContentLength::of(request.headers());
    request.extensions_mut().insert(received);
    request
}

#[cfg(test)]
mod tests {
    use crate::stack::decompression_layer::build_decompression_layer;
    use andromeda_configuration::schema::{
        content_coding::ContentCoding,
        http::decompression::{DecompressionConfig, UnacceptedEncoding},
    };
    use enumset::EnumSet;

    #[test]
    fn empty_algorithms_builds_no_layer() {
        let config = DecompressionConfig {
            algorithms: EnumSet::empty(),
            unaccepted_encoding: UnacceptedEncoding::Refuse,
        };
        assert!(build_decompression_layer(&config).is_none());
    }

    #[test]
    fn a_non_empty_selection_builds_a_layer() {
        let config = DecompressionConfig {
            algorithms: ContentCoding::Gzip | ContentCoding::Brotli,
            unaccepted_encoding: UnacceptedEncoding::PassThrough,
        };
        assert!(build_decompression_layer(&config).is_some());
    }
}
