-- ============================================================================
-- 0027: Entity Map creating-request fingerprint
-- ============================================================================
-- Clause 5.5.14: "Subsequent requests referencing an EntityMap shall use the
-- same parameters as in the original request that created the EntityMap,
-- except for the specification of Entity identifiers or parameters related
-- to pagination, or, in the case of temporal requests, the temporal query."
-- Holding a follow-up to that needs the creating request remembered beside
-- the map: an opaque `<operation>:<sha256 hex>` digest of its filter
-- parameters, written once at creation and compared for equality only, hence
-- `COLLATE "C"`. Nullable because maps created before this column existed
-- carry no digest and are accepted uncompared until they expire.
-- ============================================================================

ALTER TABLE entity_maps
    ADD COLUMN request_fingerprint TEXT COMPRESSION lz4 COLLATE "C";
