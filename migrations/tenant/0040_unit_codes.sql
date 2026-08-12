-- ============================================================================
-- 0040: UN/CEFACT unit-code catalogue
-- ============================================================================
-- `unitCode` (NGSI-LD 5.2.5) is drawn from the UN/CEFACT Recommendation 20
-- common-code list, a closed vocabulary of a few thousand short tokens. Storing
-- the token on every attribute instance repeats the same handful of bytes
-- across the whole history hypertable, so the token is interned here once and
-- the rows carry a two-byte identity instead.
--
-- The identities are not database-generated. They come from the append-only
-- assignment `crates/andromeda-unit-catalog` compiles in and 0041 seeds, which
-- is what makes an identity written by one broker readable by another: an
-- accepted code keeps its number forever, a withdrawn code keeps its row so
-- old attributes stay decodable, and a new code appends. `id` is therefore
-- plain SMALLINT with no identity clause -- nothing in this schema is allowed
-- to mint a unit-code number.
--
-- `retired_revision` records the revision that withdrew a code; a withdrawn
-- code is still resolvable, it is simply no longer offered. Conversion factors
-- are deliberately absent: the broker never converts between units, and a
-- factor column would invite it to (see `docs/unit-handling.md`).
--
-- Privileges follow 0034: this deployment has no runtime-only role, so the pair
-- below states the durable intent -- reference data any non-owner role reads
-- and cannot write, seeded and amended only by a migration running as owner.
--
-- Column order is physical: not-null fixed-width columns first in descending
-- alignment, variable-length last.
-- ============================================================================

CREATE TABLE unit_codes (
    id SMALLINT PRIMARY KEY,
    first_revision SMALLINT NOT NULL,
    retired_revision SMALLINT,
    code TEXT COLLATE "C" NOT NULL UNIQUE,
    CONSTRAINT chk_unit_codes_id_positive CHECK (id > 0),
    CONSTRAINT chk_unit_codes_code_shape CHECK (code ~ '^[A-Z0-9]{2,3}$'),
    CONSTRAINT chk_unit_codes_retired_after_first CHECK (retired_revision IS NULL OR retired_revision > first_revision)
);

REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON unit_codes FROM PUBLIC;
GRANT SELECT ON unit_codes TO PUBLIC;

-- Which source artefact the seeded assignment was rendered from. The digest is
-- what tells an operator whether two tenants were seeded from the same
-- Recommendation 20 workbook, which the revision number alone cannot.
CREATE TABLE unit_code_catalog_metadata (
    rec20_revision SMALLINT NOT NULL,
    generated_at TIMESTAMPTZ NOT NULL,
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    source_sha256 TEXT COLLATE "C" NOT NULL,
    CONSTRAINT chk_unit_code_catalog_metadata_singleton CHECK (singleton),
    CONSTRAINT chk_unit_code_catalog_metadata_revision_positive CHECK (rec20_revision > 0),
    CONSTRAINT chk_unit_code_catalog_metadata_digest_hex CHECK (source_sha256 ~ '^[0-9a-f]{64}$')
);

REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON unit_code_catalog_metadata FROM PUBLIC;
GRANT SELECT ON unit_code_catalog_metadata TO PUBLIC;
