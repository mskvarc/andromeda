-- Preserve temporal attribute-instance creation time across direct updates.

ALTER TABLE attribute_history
    ADD COLUMN modified_at TIMESTAMPTZ;
