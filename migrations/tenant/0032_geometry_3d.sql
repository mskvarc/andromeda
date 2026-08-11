-- ============================================================================
-- 032: Mixed 2D/3D Geometry Storage
-- ============================================================================
-- NGSI-LD GeoJSON coordinates may carry an altitude component (RFC 7946
-- clause 3.1.1), so a broker must be able to store XYZ geometries alongside
-- XY ones in the same column.
--
-- PostGIS enforces the dimensionality declared in a column's typmod
-- unconditionally: GEOMETRY(Geometry, 4326) rejects every geometry that has a
-- Z ordinate, and there is no typmod spelling that accepts XY and XYZ in the
-- same column. The only way to hold both is a typmod-free `geometry` column
-- with explicit CHECK constraints carrying the SRID and dimension policy that
-- the typmod used to carry.
--
-- The policy encoded below, per geometry column:
--   * ST_SRID(col) = 4326      -- WGS 84, as before
--   * ST_Zmflag(col) IN (0, 2) -- XY or XYZ; M and ZM ordinates stay rejected
--
-- The existing ST_IsValid() constraints are left untouched: OGC simple-feature
-- validity is defined on the XY projection and is unaffected by the retype.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- entities
-- ----------------------------------------------------------------------------
-- location_geo is a stored generated column derived from location, and a
-- generated column blocks retyping the column it reads. It is dropped, location
-- is retyped, and it is recreated typmod-free (a GEOGRAPHY typmod would reject
-- Z for the same reason) under its original name, as is its GiST index.
-- The remaining GiST indexes on the retyped columns are rebuilt automatically
-- by ALTER COLUMN TYPE and keep their names and partial predicates.

DROP INDEX IF EXISTS idx_entities_location_geo;

ALTER TABLE entities
    DROP COLUMN location_geo;

ALTER TABLE entities
    ALTER COLUMN location TYPE geometry,
    ALTER COLUMN observation_space TYPE geometry,
    ALTER COLUMN operation_space TYPE geometry;

ALTER TABLE entities
    ADD COLUMN location_geo geography
        GENERATED ALWAYS AS (geom_to_geog(location)) STORED;

CREATE INDEX idx_entities_location_geo ON entities USING GIST (location_geo);

ALTER TABLE entities
    ADD CONSTRAINT chk_entities_location_srid
        CHECK (location IS NULL OR ST_SRID(location) = 4326),
    ADD CONSTRAINT chk_entities_location_zm
        CHECK (location IS NULL OR ST_Zmflag(location) IN (0, 2)),
    ADD CONSTRAINT chk_entities_observation_space_srid
        CHECK (observation_space IS NULL OR ST_SRID(observation_space) = 4326),
    ADD CONSTRAINT chk_entities_observation_space_zm
        CHECK (observation_space IS NULL OR ST_Zmflag(observation_space) IN (0, 2)),
    ADD CONSTRAINT chk_entities_operation_space_srid
        CHECK (operation_space IS NULL OR ST_SRID(operation_space) = 4326),
    ADD CONSTRAINT chk_entities_operation_space_zm
        CHECK (operation_space IS NULL OR ST_Zmflag(operation_space) IN (0, 2));

-- ----------------------------------------------------------------------------
-- attribute_current
-- ----------------------------------------------------------------------------
-- idx_ac_attr_geo is a composite btree_gist index over
-- (attribute_name_id, value_geo); it is rebuilt in place by the retype.

ALTER TABLE attribute_current
    ALTER COLUMN value_geo TYPE geometry;

ALTER TABLE attribute_current
    ADD CONSTRAINT chk_attribute_current_value_geo_srid
        CHECK (value_geo IS NULL OR ST_SRID(value_geo) = 4326),
    ADD CONSTRAINT chk_attribute_current_value_geo_zm
        CHECK (value_geo IS NULL OR ST_Zmflag(value_geo) IN (0, 2));

-- ----------------------------------------------------------------------------
-- context_source_registrations
-- ----------------------------------------------------------------------------

ALTER TABLE context_source_registrations
    ALTER COLUMN location TYPE geometry,
    ALTER COLUMN observation_space TYPE geometry,
    ALTER COLUMN operation_space TYPE geometry;

ALTER TABLE context_source_registrations
    ADD CONSTRAINT chk_csr_location_srid
        CHECK (location IS NULL OR ST_SRID(location) = 4326),
    ADD CONSTRAINT chk_csr_location_zm
        CHECK (location IS NULL OR ST_Zmflag(location) IN (0, 2)),
    ADD CONSTRAINT chk_csr_observation_space_srid
        CHECK (observation_space IS NULL OR ST_SRID(observation_space) = 4326),
    ADD CONSTRAINT chk_csr_observation_space_zm
        CHECK (observation_space IS NULL OR ST_Zmflag(observation_space) IN (0, 2)),
    ADD CONSTRAINT chk_csr_operation_space_srid
        CHECK (operation_space IS NULL OR ST_SRID(operation_space) = 4326),
    ADD CONSTRAINT chk_csr_operation_space_zm
        CHECK (operation_space IS NULL OR ST_Zmflag(operation_space) IN (0, 2));

-- ----------------------------------------------------------------------------
-- attribute_history
-- ----------------------------------------------------------------------------
-- attribute_history is a compressed TimescaleDB hypertable. A column retype is
-- refused while any chunk is in columnar form and while columnar storage is
-- enabled on the hypertable, so the policy is removed, every chunk is returned
-- to rowstore, columnar storage is switched off, the column is retyped, and the
-- columnar settings and policy are restored exactly as 0008 leaves them.
--
-- Both the columnstore API (convert_to_rowstore, add_columnstore_policy) and
-- legacy compression (decompress_chunk, add_compression_policy) are handled;
-- which one is in use is probed from pg_proc, mirroring 0008.

DO $$
DECLARE
    has_columnstore BOOLEAN;
    columnar_chunk REGCLASS;
BEGIN
    SELECT EXISTS(SELECT 1 FROM pg_proc WHERE proname = 'add_columnstore_policy')
    INTO has_columnstore;

    IF has_columnstore THEN
        RAISE NOTICE 'columnstore API available -- using columnstore';

        CALL remove_columnstore_policy('attribute_history', if_exists => TRUE);

        FOR columnar_chunk IN
            SELECT format('%I.%I', c.chunk_schema, c.chunk_name)::regclass
            FROM timescaledb_information.chunks AS c
            WHERE c.hypertable_name = 'attribute_history' AND c.is_compressed
        LOOP
            CALL convert_to_rowstore(columnar_chunk);
        END LOOP;

        ALTER TABLE attribute_history SET (timescaledb.enable_columnstore = FALSE);
    ELSE
        RAISE NOTICE 'columnstore API not available -- using legacy compression';

        PERFORM remove_compression_policy('attribute_history', if_exists => TRUE);

        PERFORM decompress_chunk(chunk, if_compressed => TRUE)
        FROM show_chunks('attribute_history') AS chunk;

        ALTER TABLE attribute_history SET (timescaledb.compress = FALSE);
    END IF;

    ALTER TABLE attribute_history
        ALTER COLUMN value_geo TYPE geometry;

    ALTER TABLE attribute_history
        ADD CONSTRAINT chk_attribute_history_value_geo_srid
            CHECK (value_geo IS NULL OR ST_SRID(value_geo) = 4326),
        ADD CONSTRAINT chk_attribute_history_value_geo_zm
            CHECK (value_geo IS NULL OR ST_Zmflag(value_geo) IN (0, 2));

    IF has_columnstore THEN
        ALTER TABLE attribute_history SET (
            timescaledb.enable_columnstore,
            timescaledb.segmentby = 'tea_id',
            timescaledb.orderby = 'time DESC'
        );
        CALL add_columnstore_policy('attribute_history', after => INTERVAL '7 days');
        SET timescaledb.max_tuples_decompressed_per_dml_transaction = 1000000;
    ELSE
        ALTER TABLE attribute_history SET (
            timescaledb.compress,
            timescaledb.compress_segmentby = 'tea_id',
            timescaledb.compress_orderby = 'time DESC'
        );
        PERFORM add_compression_policy('attribute_history', INTERVAL '7 days');
    END IF;
END
$$;
