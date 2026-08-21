-- ============================================================================
-- 0023: Context Source Registration datasetId
-- ============================================================================
-- Table 5.2.9-1 gives a registration an optional `datasetId` member: the
-- datasetIds of the attribute instances the Context Source can provide, each a
-- URI or the `@none` token naming the default instance (clause 4.5.5). The
-- member was previously carried in `additional_properties`; it is a typed,
-- specification-defined member and gets its own column. No index: nothing
-- preselects registrations by dataset.
-- ============================================================================

ALTER TABLE context_source_registrations
    ADD COLUMN dataset_id TEXT[] COMPRESSION lz4;
