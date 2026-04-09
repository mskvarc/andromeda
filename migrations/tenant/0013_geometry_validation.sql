-- ============================================================================
-- 013: Geometry Validation Constraints and Missing Indexes
-- ============================================================================
-- Adds CHECK constraints to ensure all stored geometry is valid, and creates
-- missing GIST indexes for observation_space and operation_space columns.
--
-- Context:
--   Invalid GeoJSON (self-intersecting polygons, unclosed rings) can corrupt
--   spatial query results. The application layer now wraps all geometry
--   ingestion with ST_MakeValid(), but CHECK constraints serve as a safety net.
--
--   Migration 0010 added observation_space and operation_space columns to
--   entities but omitted GIST indexes. This migration adds them for complete
--   spatial query coverage.
--
-- IMPORTANT: Before running this migration on existing deployments with data,
-- run the remediation queries to fix any pre-existing invalid geometries:
--
--   UPDATE entities SET location = ST_MakeValid(location)
--       WHERE location IS NOT NULL AND NOT ST_IsValid(location);
--   UPDATE entities SET observation_space = ST_MakeValid(observation_space)
--       WHERE observation_space IS NOT NULL AND NOT ST_IsValid(observation_space);
--   UPDATE entities SET operation_space = ST_MakeValid(operation_space)
--       WHERE operation_space IS NOT NULL AND NOT ST_IsValid(operation_space);
--   UPDATE context_source_registrations SET location = ST_MakeValid(location)
--       WHERE location IS NOT NULL AND NOT ST_IsValid(location);
--   UPDATE context_source_registrations SET observation_space = ST_MakeValid(observation_space)
--       WHERE observation_space IS NOT NULL AND NOT ST_IsValid(observation_space);
--   UPDATE context_source_registrations SET operation_space = ST_MakeValid(operation_space)
--       WHERE operation_space IS NOT NULL AND NOT ST_IsValid(operation_space);
--   UPDATE attribute_history SET value_geo = ST_MakeValid(value_geo)
--       WHERE value_geo IS NOT NULL AND NOT ST_IsValid(value_geo);
-- ============================================================================

-- ----------------------------------------------------------------------------
-- CHECK Constraints: Geometry Validity
-- ----------------------------------------------------------------------------
-- These constraints enforce that only valid geometry is stored. With
-- ST_MakeValid() applied at ingestion, these should never fire in practice,
-- but serve as a defensive layer against application bugs or direct SQL
-- modifications.

ALTER TABLE entities
    ADD CONSTRAINT chk_entities_location_valid
        CHECK (location IS NULL OR ST_IsValid(location)),
    ADD CONSTRAINT chk_entities_observation_space_valid
        CHECK (observation_space IS NULL OR ST_IsValid(observation_space)),
    ADD CONSTRAINT chk_entities_operation_space_valid
        CHECK (operation_space IS NULL OR ST_IsValid(operation_space));

ALTER TABLE context_source_registrations
    ADD CONSTRAINT chk_csr_location_valid
        CHECK (location IS NULL OR ST_IsValid(location)),
    ADD CONSTRAINT chk_csr_observation_space_valid
        CHECK (observation_space IS NULL OR ST_IsValid(observation_space)),
    ADD CONSTRAINT chk_csr_operation_space_valid
        CHECK (operation_space IS NULL OR ST_IsValid(operation_space));

ALTER TABLE attribute_history
    ADD CONSTRAINT chk_attribute_history_value_geo_valid
        CHECK (value_geo IS NULL OR ST_IsValid(value_geo));

-- ----------------------------------------------------------------------------
-- Missing GIST Indexes
-- ----------------------------------------------------------------------------
-- Migration 0010 added observation_space and operation_space columns to
-- entities but did not create indexes. Without these, spatial queries
-- filtering by these columns perform sequential scans.

CREATE INDEX idx_entities_observation_space ON entities USING GIST (observation_space)
    WHERE observation_space IS NOT NULL;

CREATE INDEX idx_entities_operation_space ON entities USING GIST (operation_space)
    WHERE operation_space IS NOT NULL;

-- Index for GeoProperty values in temporal history. Enables efficient
-- spatial queries on historical attribute values.
CREATE INDEX idx_attribute_history_value_geo ON attribute_history USING GIST (value_geo)
    WHERE value_geo IS NOT NULL;
