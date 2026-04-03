//! Shared query parameter trait for entity, temporal, and CSR query types.

/// Generate a trait method that splits a raw comma-separated parameter into a `Vec<String>`.
///
/// The generated method returns an empty vector when the parameter is absent.
macro_rules! csv_parser {
    ($method:ident, $raw:ident, $doc:expr) => {
        #[doc = $doc]
        ///
        /// Returns an empty vector if the parameter is absent.
        fn $method(&self) -> Vec<String> {
            self.$raw()
                .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
                .unwrap_or_default()
        }
    };
}

/// Shared query parameter access for entity, temporal, and CSR query types.
///
/// Both `EntityQueryParameters` and `TemporalQueryParameters` (and partially
/// `CsrQueryParameters`) share a common set of fields: `id`, `attrs`, `pick`,
/// `omit`, `datasetId`, `limit`, and `offset`. This trait provides a uniform
/// interface with default parsing implementations that split comma-separated
/// values and apply default/cap logic to pagination parameters.
pub trait SharedQueryParams {
    /// Returns the raw `id` query parameter value, if present.
    fn raw_id(&self) -> Option<&str>;

    /// Returns the raw `attrs` query parameter value, if present.
    fn raw_attrs(&self) -> Option<&str>;

    /// Returns the raw `pick` query parameter value, if present.
    fn raw_pick(&self) -> Option<&str>;

    /// Returns the raw `omit` query parameter value, if present.
    fn raw_omit(&self) -> Option<&str>;

    /// Returns the raw `datasetId` query parameter value, if present.
    fn raw_dataset_id(&self) -> Option<&str>;

    /// Returns the raw `limit` query parameter value, if present.
    fn raw_limit(&self) -> Option<u32>;

    /// Returns the raw `offset` query parameter value, if present.
    fn raw_offset(&self) -> Option<u32>;

    /// Parses the `id` parameter as a comma-separated list of entity identifiers.
    ///
    /// Returns `None` if the `id` parameter is absent, or `Some(Vec)` with the
    /// trimmed individual identifiers.
    fn parse_ids(&self) -> Option<Vec<String>> {
        self.raw_id().map(|s| s.split(',').map(|id| id.trim().to_string()).collect())
    }

    csv_parser!(
        parse_attrs,
        raw_attrs,
        "Parses the `attrs` parameter as a comma-separated list of attribute names."
    );
    csv_parser!(
        parse_pick,
        raw_pick,
        "Parses the `pick` parameter as a comma-separated list of attribute names to include (NGSI-LD 6.3.15)."
    );
    csv_parser!(
        parse_omit,
        raw_omit,
        "Parses the `omit` parameter as a comma-separated list of attribute names to exclude (NGSI-LD 6.3.15)."
    );
    csv_parser!(
        parse_dataset_ids,
        raw_dataset_id,
        "Parses the `datasetId` parameter as a comma-separated list of dataset identifiers."
    );

    /// Returns the effective pagination limit, capped at 1000.
    ///
    /// Defaults to 20 when the `limit` parameter is absent.
    /// The upper cap of 1000 prevents excessively large result sets.
    fn effective_limit(&self) -> u32 {
        self.raw_limit().unwrap_or(20).min(1000)
    }

    /// Returns the effective pagination limit without any upper cap.
    ///
    /// Defaults to `u32::MAX` when the `limit` parameter is absent.
    /// Used internally when the caller manages its own result-size constraints.
    fn effective_limit_uncapped(&self) -> u32 {
        self.raw_limit().unwrap_or(u32::MAX)
    }

    /// Returns the effective pagination offset.
    ///
    /// Defaults to 0 when the `offset` parameter is absent.
    fn effective_offset(&self) -> u32 {
        self.raw_offset().unwrap_or(0)
    }
}

/// Checks whether a string is a valid URI (either a URN or URL).
///
/// Used by entity and CSR query parameter validation to verify that
/// entity IDs are well-formed identifiers per NGSI-LD 4.5.1.
pub(crate) fn is_valid_uri(id: &str) -> bool {
    if id.starts_with("urn:") {
        return id.parse::<urn::Urn>().is_ok();
    }
    url::Url::parse(id).is_ok()
}
