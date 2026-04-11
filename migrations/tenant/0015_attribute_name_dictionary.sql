-- ============================================================================
-- 015: Attribute Name Dictionary — Normalize TEA attribute names
-- ============================================================================
-- For existing deployments: migrates temporal_entity_attributes.attribute_name
-- (TEXT) to attribute_name_id (SMALLINT) referencing a new dictionary table.
--
-- Fresh databases already have the normalized schema from the rewritten 0005.
-- This migration is idempotent: it checks for the old column before acting.
-- ============================================================================

DO $$
BEGIN
    -- Only run if the old TEXT column still exists (i.e. this is an upgrade,
    -- not a fresh install where 0005 already created the normalized schema).
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'temporal_entity_attributes'
        AND column_name = 'attribute_name'
        AND data_type = 'text'
    ) THEN
        -- Create the dictionary table if it does not yet exist.
        CREATE TABLE IF NOT EXISTS attribute_names (
            id SMALLINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
            name TEXT NOT NULL,
            CONSTRAINT uq_attribute_names_name UNIQUE (name)
        );

        -- Populate dictionary from existing distinct values.
        INSERT INTO attribute_names (name)
        SELECT DISTINCT attribute_name FROM temporal_entity_attributes
        ON CONFLICT (name) DO NOTHING;

        -- Add the FK column.
        ALTER TABLE temporal_entity_attributes
            ADD COLUMN attribute_name_id SMALLINT;

        -- Backfill from dictionary.
        UPDATE temporal_entity_attributes tea
        SET attribute_name_id = an.id
        FROM attribute_names an
        WHERE an.name = tea.attribute_name;

        -- Make non-nullable.
        ALTER TABLE temporal_entity_attributes
            ALTER COLUMN attribute_name_id SET NOT NULL;

        -- Add FK constraint.
        ALTER TABLE temporal_entity_attributes
            ADD CONSTRAINT fk_tea_attribute_name
            FOREIGN KEY (attribute_name_id) REFERENCES attribute_names(id);

        -- Drop old unique constraint and create new one.
        ALTER TABLE temporal_entity_attributes
            DROP CONSTRAINT IF EXISTS temporal_entity_attributes_entity_id_attribute_name_datase_key;
        ALTER TABLE temporal_entity_attributes
            ADD CONSTRAINT temporal_entity_attributes_entity_id_attribute_name_id_dat_key
            UNIQUE NULLS NOT DISTINCT (entity_id, attribute_name_id, dataset_id);

        -- Drop the old TEXT column.
        ALTER TABLE temporal_entity_attributes
            DROP COLUMN attribute_name;
    END IF;
END
$$;
