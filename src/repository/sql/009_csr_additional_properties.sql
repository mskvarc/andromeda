-- ============================================================================
-- 009: CSR additional properties
-- ============================================================================
-- Stores non-standard properties from CSR payloads (e.g. custom attributes
-- like "csourceProperty1") so they can be returned in query responses and
-- filtered with the `q` query parameter.
-- ============================================================================

ALTER TABLE context_source_registrations
    ADD COLUMN IF NOT EXISTS additional_properties JSONB;
