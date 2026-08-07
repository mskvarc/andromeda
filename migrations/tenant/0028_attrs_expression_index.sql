-- 0028: Index the sole remaining payload-root key predicate.
--
-- `attrs=` compiles to `payload -> 0 ? $n`; a GIN index on the bare payload
-- column cannot serve that expression. This index deliberately pays the extra
-- ingest write cost only for that currently supported predicate.
CREATE INDEX idx_entities_payload_root_keys ON entities USING GIN ((payload -> 0));
