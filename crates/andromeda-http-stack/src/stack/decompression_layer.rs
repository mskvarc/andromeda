//! Mapping of the decompression configuration onto a `tower-http` layer.

use crate::config::{content_coding::ContentCoding, decompression::DecompressionConfig};
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
    Some(layer.pass_through_unaccepted(config.pass_through_unaccepted))
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{content_coding::ContentCoding, decompression::DecompressionConfig},
        stack::decompression_layer::build_decompression_layer,
    };
    use enumset::EnumSet;

    #[test]
    fn empty_algorithms_builds_no_layer() {
        let config = DecompressionConfig {
            algorithms: EnumSet::empty(),
            pass_through_unaccepted: false,
        };
        assert!(build_decompression_layer(&config).is_none());
    }

    #[test]
    fn a_non_empty_selection_builds_a_layer() {
        let config = DecompressionConfig {
            algorithms: ContentCoding::Gzip | ContentCoding::Brotli,
            pass_through_unaccepted: true,
        };
        assert!(build_decompression_layer(&config).is_some());
    }
}
