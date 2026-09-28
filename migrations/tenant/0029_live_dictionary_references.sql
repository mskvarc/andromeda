-- ============================================================================
-- 0029: Live dictionary references
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.6.1.4, 5.16.1.4
--
-- The foreign keys from the live relations to `attribute_names`,
-- `entity_type_names` and `unit_codes`. They exist on the live relations only,
-- never on a Snapshot schema's copies.
--
-- A foreign key locks both of its ends. Declaring one takes `SHARE ROW
-- EXCLUSIVE` on the referenced table, and dropping one, which every
-- `DROP SCHEMA ... CASCADE` of a Snapshot schema does, takes `ACCESS
-- EXCLUSIVE` on it. On a Snapshot's copies these keys would make every
-- Snapshot creation, promotion, expiry and deletion lock the dictionaries
-- every live write reads, and PostgreSQL resolves the resulting cycles by
-- aborting one side: a live write failing because an unrelated Snapshot was
-- being prepared or dropped. Clause 5.6.1.4 fails a create only with the
-- errors it lists, and clause 5.16.1.4 prepares a Snapshot in the
-- background of the live context, so Snapshot DDL must never stall or abort a
-- live write.
--
-- A Snapshot's copies lose no integrity by the omission. The dictionaries
-- never delete or re-key a row, and a Snapshot is cloned from live rows whose
-- keys these constraints have already checked.
-- ============================================================================

ALTER TABLE temporal_attribute_streams
    ADD CONSTRAINT fk_temporal_attribute_streams_attribute_name
        FOREIGN KEY (attribute_name_key)
        REFERENCES attribute_names(attribute_name_key);

ALTER TABLE attribute_history_instance_claims
    ADD CONSTRAINT fk_attribute_history_instance_claims_attribute_name
        FOREIGN KEY (attribute_name_key)
        REFERENCES attribute_names(attribute_name_key);

ALTER TABLE entity_type_members
    ADD CONSTRAINT fk_entity_type_members_entity_type_name
        FOREIGN KEY (entity_type_name_key)
        REFERENCES entity_type_names(entity_type_name_key);

ALTER TABLE current_attribute_values
    ADD CONSTRAINT fk_current_attribute_values_attribute_name
        FOREIGN KEY (attribute_name_key)
        REFERENCES attribute_names(attribute_name_key),
    ADD CONSTRAINT fk_current_attribute_values_unit_code
        FOREIGN KEY (unit_code_id)
        REFERENCES unit_codes(id);
