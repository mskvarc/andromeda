-- Runs once on first init of the postgres-c server (mounted into
-- /docker-entrypoint-initdb.d). This server backs exactly one broker identity
-- (broker-c and every replica of it); the broker migrates its own schema on
-- startup and auto-creates its per-tenant databases (tenant_db_prefix) as needed.
CREATE DATABASE broker_c;
