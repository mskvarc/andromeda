-- Runs once on first Postgres init (mounted into /docker-entrypoint-initdb.d).
-- One base database per broker instance. Each broker migrates its own schema on
-- startup and auto-creates its per-tenant databases (tenant_db_prefix) as needed.
CREATE DATABASE broker_a;
CREATE DATABASE broker_b;
CREATE DATABASE broker_c;
