-- Migration 0016: Index and extension optimizations
--
-- 1. pg_stat_statements (graceful fallback if not in shared_preload_libraries)
-- 2. pg_trgm extension + trigram index on entity IDs for idPattern regex queries
-- 3. Partial GIST indexes (exclude NULL geometry rows to reduce index size)
-- 4. Native geography column for entities.location (avoids runtime geometry->geography cast)

-- 1. pg_stat_statements
DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'pg_stat_statements requires shared_preload_libraries -- skipping';
END
$$;

-- 2. pg_trgm + trigram index
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX idx_entities_id_trgm ON entities USING GIN (id gin_trgm_ops);

-- 3. Partial GIST indexes (drop non-partial, recreate with WHERE clause)

-- entities.location (originally in migration 0003)
DROP INDEX IF EXISTS idx_entities_location;
CREATE INDEX idx_entities_location ON entities USING GIST (location)
    WHERE location IS NOT NULL;

-- CSR indexes (originally in migration 0008)
DROP INDEX IF EXISTS idx_csr_location;
CREATE INDEX idx_csr_location ON context_source_registrations USING GIST (location)
    WHERE location IS NOT NULL;

DROP INDEX IF EXISTS idx_csr_observation_space;
CREATE INDEX idx_csr_observation_space ON context_source_registrations USING GIST (observation_space)
    WHERE observation_space IS NOT NULL;

DROP INDEX IF EXISTS idx_csr_operation_space;
CREATE INDEX idx_csr_operation_space ON context_source_registrations USING GIST (operation_space)
    WHERE operation_space IS NOT NULL;

-- 4. Native geography column for entities.location
--
-- Immutable wrapper for geometry->geography cast.
-- Technically STABLE in PostGIS (depends on spatial_ref_sys), but deterministic
-- for SRID 4326. Marking IMMUTABLE enables GENERATED ALWAYS AS usage.
-- This is a well-established PostGIS pattern for stored generated columns.
CREATE OR REPLACE FUNCTION geom_to_geog(g geometry)
RETURNS geography
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE
AS $$ SELECT g::geography $$;

ALTER TABLE entities
    ADD COLUMN location_geo GEOGRAPHY(Geometry, 4326)
    GENERATED ALWAYS AS (geom_to_geog(location)) STORED;

CREATE INDEX idx_entities_location_geo ON entities USING GIST (location_geo);
