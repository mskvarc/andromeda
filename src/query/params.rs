/// Shared query parameter access for entity and temporal query types.
///
/// Both `EntityQueryParameters` and `TemporalQueryParameters` share a common
/// set of fields (id, type, attrs, pick, omit, datasetId, limit, offset).
/// This trait provides a uniform interface with default parsing implementations.
pub trait SharedQueryParams {
    fn raw_id(&self) -> Option<&str>;
    fn raw_entity_type(&self) -> Option<&str>;
    fn raw_attrs(&self) -> Option<&str>;
    fn raw_pick(&self) -> Option<&str>;
    fn raw_omit(&self) -> Option<&str>;
    fn raw_dataset_id(&self) -> Option<&str>;
    fn raw_limit(&self) -> Option<u32>;
    fn raw_offset(&self) -> Option<u32>;

    fn parse_ids(&self) -> Option<Vec<String>> {
        self.raw_id().map(|s| s.split(',').map(|id| id.trim().to_string()).collect())
    }

    fn parse_types(&self) -> Vec<String> {
        self.raw_entity_type()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_default()
    }

    fn parse_attrs(&self) -> Vec<String> {
        self.raw_attrs()
            .map(|s| s.split(',').map(|a| a.trim().to_string()).collect())
            .unwrap_or_default()
    }

    fn parse_pick(&self) -> Vec<String> {
        self.raw_pick()
            .map(|s| s.split(',').map(|p| p.trim().to_string()).collect())
            .unwrap_or_default()
    }

    fn parse_omit(&self) -> Vec<String> {
        self.raw_omit()
            .map(|s| s.split(',').map(|o| o.trim().to_string()).collect())
            .unwrap_or_default()
    }

    fn parse_dataset_ids(&self) -> Vec<String> {
        self.raw_dataset_id()
            .map(|s| s.split(',').map(|d| d.trim().to_string()).collect())
            .unwrap_or_default()
    }

    fn effective_limit(&self) -> u32 {
        self.raw_limit().unwrap_or(20).min(1000)
    }

    fn effective_limit_uncapped(&self) -> u32 {
        self.raw_limit().unwrap_or(u32::MAX)
    }

    fn effective_offset(&self) -> u32 {
        self.raw_offset().unwrap_or(0)
    }
}
