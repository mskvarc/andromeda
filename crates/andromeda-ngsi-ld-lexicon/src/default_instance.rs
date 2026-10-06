//! The keyword naming the default attribute instance (clause 4.5.5).

/// The reserved `datasetId` value naming the default attribute instance.
///
/// Clause 4.5.5: an attribute instance with no `datasetId`, or with
/// `"datasetId": "@none"`, is the default instance, and `datasetId=@none`
/// selects it. `@none` is a JSON-LD keyword, not a URI, so it cannot collide
/// with a real identifier.
pub const DEFAULT_INSTANCE_TOKEN: &str = "@none";
