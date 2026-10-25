//! Folding this process's un-flushed context accesses into a served count.

use andromeda_jsonld::hit_counter::HitCounter;
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;

/// Add the accesses this process has counted but not yet flushed to `metadata`.
///
/// Clause 5.13.3.5 defines `numberOfHits` as "number of times the @context was
/// found in the storage", which is a fact about the accesses that have happened
/// rather than about the ones that have been written down. The resolver
/// accumulates them per URL and a background task folds them into the stored row
/// in batches, so a client reading the metadata in between would otherwise be
/// told a number that is already out of date.
///
/// The fold keys on [`ContextMetadata::serving_iri`], the raw serving URL exactly
/// as it is stored, because that is the string the counter is bumped and flushed
/// under. Keying on the re-serialized [`ContextMetadata::url`] would silently
/// miss: `url::Url` normalizes a trailing slash onto an authority-only URL,
/// strips a default port, and re-encodes segments, so for any serving URL that
/// does not round-trip the fold would look up a key the counter never holds and
/// the served count would stay stale until the timer flush.
pub(crate) fn fold_pending_hits(counter: &HitCounter, metadata: &mut ContextMetadata) {
    let Some(serving_iri) = metadata.serving_iri.as_ref() else {
        return;
    };
    metadata.number_of_hits = metadata.number_of_hits.saturating_add(counter.pending(serving_iri));
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::pending_hits::fold_pending_hits;
    use andromeda_jsonld::hit_counter::HitCounter;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::jsonld_context::{ContextKind, ContextMetadata};
    use chrono::Utc;
    use iri_rs::IriBuf;
    use url::Url;

    /// Build a metadata record carrying `serving_iri` and a base hit count.
    fn metadata_with(serving: &IriBuf, base_hits: u64) -> ContextMetadata {
        let now = Utc::now();
        ContextMetadata::builder()
            .url(Url::parse(serving.as_str()).ok())
            .serving_iri(Some(serving.clone())) // clone: the counter key is owned by the metadata record under test.
            .local_id(serving.as_str().parse::<Identifier>().expect("a serving URL is a valid URI"))
            .kind(ContextKind::Cached)
            .created_at(now)
            .last_usage(now)
            .number_of_hits(base_hits)
            .build()
    }

    /// The fold attributes the pending count to the raw serving URL for URL
    /// shapes that `url::Url` would rewrite, so the served `numberOfHits` reflects
    /// this process's un-flushed accesses immediately. The three shapes are the
    /// ones normalization changes: an authority-only URL (a trailing slash is
    /// added), a default port (it is stripped), and a percent-encoded segment.
    #[test]
    fn the_fold_keys_on_the_raw_serving_url_even_when_it_does_not_round_trip() {
        for raw in [
            "https://example.com",
            "https://example.com:443/context.jsonld",
            "https://example.com/a%2Fb/context.jsonld",
        ] {
            let serving = IriBuf::new(raw.to_owned()).expect("a serving URL is a valid IRI");
            let counter = HitCounter::new();
            for _ in 0..3 {
                counter.bump(&serving);
            }

            let mut metadata = metadata_with(&serving, 10);
            fold_pending_hits(&counter, &mut metadata);
            assert_eq!(metadata.number_of_hits, 13, "the raw serving URL {raw} must fold its three pending hits");
        }
    }

    /// A record whose stored serving URL does not survive `url::Url`
    /// re-serialization proves the counter would be missed if the fold keyed on
    /// the parsed URL: the normalized string differs from the raw one, so a
    /// lookup by it returns nothing.
    #[test]
    fn a_normalized_url_key_would_miss_the_counter_the_raw_key_finds() {
        let raw = "https://example.com";
        let serving = IriBuf::new(raw.to_owned()).expect("a serving URL is a valid IRI");
        let normalized = Url::parse(raw).expect("the raw string parses as a URL").to_string();
        assert_ne!(normalized, raw, "url::Url must normalize an authority-only URL, or the fix is unnecessary");

        let counter = HitCounter::new();
        counter.bump(&serving);
        let normalized_key = IriBuf::new(normalized).expect("the normalized URL is a valid IRI");
        assert_eq!(counter.pending(&normalized_key), 0, "the normalized key finds nothing");
        assert_eq!(counter.pending(&serving), 1, "the raw key finds the bump");
    }

    /// A record with no serving URL folds nothing and leaves the stored count.
    #[test]
    fn a_record_without_a_serving_url_is_left_at_its_stored_count() {
        let counter = HitCounter::new();
        let now = Utc::now();
        let mut metadata = ContextMetadata::builder()
            .serving_iri(None)
            .local_id("urn:ngsi-ld:JsonLdContext:hosted".parse::<Identifier>().expect("a URN is a valid URI"))
            .kind(ContextKind::Hosted)
            .created_at(now)
            .last_usage(now)
            .number_of_hits(7)
            .build();
        fold_pending_hits(&counter, &mut metadata);
        assert_eq!(metadata.number_of_hits, 7, "no serving URL means no fold");
    }
}
