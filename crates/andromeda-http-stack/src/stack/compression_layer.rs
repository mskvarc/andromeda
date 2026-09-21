//! Mapping of the compression configuration onto a `tower-http` compression layer.

use crate::config::{
    compression::{CompressionConfig, CompressionQuality},
    content_coding::ContentCoding,
};
use tower_http::compression::{CompressionLayer, CompressionLevel};

/// Build the response-compression layer, or `None` when no coding is enabled.
///
/// The layer starts with every coding on and each absent coding is switched off,
/// so the enabled set is exactly `config.algorithms`. An empty set means no
/// compression at all, and no layer is installed.
pub(crate) fn build_compression_layer(config: &CompressionConfig) -> Option<CompressionLayer> {
    if config.algorithms.is_empty() {
        return None;
    }
    let mut layer = CompressionLayer::new();
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
    Some(layer.quality(compression_level(config.quality)))
}

/// Translate the configured quality onto the layer's compression level.
const fn compression_level(quality: CompressionQuality) -> CompressionLevel {
    match quality {
        CompressionQuality::Fastest => CompressionLevel::Fastest,
        CompressionQuality::Best => CompressionLevel::Best,
        CompressionQuality::Default => CompressionLevel::Default,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{
            compression::{CompressionConfig, CompressionQuality},
            content_coding::ContentCoding,
        },
        stack::compression_layer::{build_compression_layer, compression_level},
    };
    use enumset::EnumSet;
    use tower_http::compression::CompressionLevel;

    #[test]
    fn empty_algorithms_builds_no_layer() {
        let config = CompressionConfig {
            algorithms: EnumSet::empty(),
            quality: CompressionQuality::Default,
        };
        assert!(build_compression_layer(&config).is_none());
    }

    #[test]
    fn a_non_empty_selection_builds_a_layer() {
        let config = CompressionConfig {
            algorithms: ContentCoding::Gzip | ContentCoding::Zstd,
            quality: CompressionQuality::Best,
        };
        assert!(build_compression_layer(&config).is_some());
    }

    #[test]
    fn quality_maps_to_expected_compression_level() {
        assert!(matches!(compression_level(CompressionQuality::Fastest), CompressionLevel::Fastest));
        assert!(matches!(compression_level(CompressionQuality::Best), CompressionLevel::Best));
        assert!(matches!(compression_level(CompressionQuality::Default), CompressionLevel::Default));
    }
}
