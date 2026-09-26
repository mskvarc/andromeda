-- ============================================================================
-- 0029: Dictionary references of the live branch
-- ============================================================================
-- The live relations the branch files create carry `attribute_names` keys
-- (temporal streams, verbatim-instanceId claims, the current-value projection)
-- and `unit_codes` identities (the current-value projection). Their foreign
-- keys are declared here, on the live relations only, and never in a branch
-- file.
--
-- A foreign key locks both of its ends. Declaring one takes SHARE ROW
-- EXCLUSIVE on the referenced table, and dropping it -- which every
-- `DROP SCHEMA ... CASCADE` of a Snapshot schema does -- takes ACCESS
-- EXCLUSIVE on it. Were the branch files to declare these keys, every
-- Snapshot creation, promotion, expiry and deletion would lock the two
-- tenant-global dictionaries that every live write reads under ROW SHARE, and
-- PostgreSQL resolves the resulting cycles by aborting one side: a live
-- write failing because an unrelated Snapshot was being prepared or dropped.
-- Clause 5.6.1.4 gives a create exactly one failure, `AlreadyExists`, and
-- clause 5.16.1.4 prepares a Snapshot in the background of the live context,
-- so Snapshot DDL must never be able to stall or abort a live write. With the
-- keys here, a Snapshot schema's DDL touches only relations inside it.
--
-- A Snapshot branch loses no integrity by the omission. Both dictionaries are
-- append-only -- no statement deletes or re-keys a row -- and a branch is
-- cloned from the live relations, whose keys these constraints have already
-- checked, so a branch key always names a row that exists.
--
-- This file is in `migrations/tenant/` and only ever runs in `public`, after
-- the branch set has created the live relations it alters. Each relation is
-- still empty when it runs, so the constraints are validated on no rows.
-- ============================================================================

ALTER TABLE temporal_attribute_streams
    ADD CONSTRAINT fk_temporal_attribute_streams_attribute_name
        FOREIGN KEY (attribute_name_id)
        REFERENCES attribute_names(id);

ALTER TABLE attribute_history_instance_claims
    ADD CONSTRAINT fk_attribute_history_instance_claims_attribute_name
        FOREIGN KEY (attribute_name_id)
        REFERENCES attribute_names(id);

ALTER TABLE current_attribute_values
    ADD CONSTRAINT fk_current_attribute_values_attribute_name
        FOREIGN KEY (attribute_name_id)
        REFERENCES attribute_names(id),
    ADD CONSTRAINT fk_current_attribute_values_unit_code
        FOREIGN KEY (unit_code_id)
        REFERENCES unit_codes(id);
