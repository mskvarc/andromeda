//! Hand-written inputs of the `datasetId` of clause 4.5.5.

/// `datasetId` values of clause 4.5.5.
pub(super) const DATASET_SELECTOR_INPUTS: &[&str] = &[
    "@none",
    "urn:ngsi-ld:Dataset:1",
    "urn:x:1",
    "http://example.com/d",
    "",
    "dataset",
    "@nonex",
    " urn:x:1",
    "urn:x:1 ",
    "@None",
    "urn:",
    "https://example.com/a b",
    "é:x",
    "urn:x:é",
];
