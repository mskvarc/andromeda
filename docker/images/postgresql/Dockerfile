# syntax=docker/dockerfile:1

FROM postgres:18.6-trixie

RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl gnupg; \
    curl -fsSL https://packagecloud.io/timescale/timescaledb/gpgkey \
        | gpg --dearmor -o /usr/share/keyrings/timescaledb.gpg; \
    . /etc/os-release; \
    echo "deb [signed-by=/usr/share/keyrings/timescaledb.gpg] https://packagecloud.io/timescale/timescaledb/debian/ ${VERSION_CODENAME} main" \
        > /etc/apt/sources.list.d/timescaledb.list; \
    apt-get purge -y --auto-remove curl gnupg; \
    rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    apt-get update; \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        postgresql-18-postgis-3 \
        postgresql-18-postgis-3-scripts \
        timescaledb-2-postgresql-18 \
        timescaledb-2-loader-postgresql-18 \
        timescaledb-tools; \
    rm -rf /var/lib/apt/lists/*

# apache: strip the TSL module and skip the TSL-licensed toolkit.
ARG TIMESCALEDB_EDITION=tsl
RUN set -eux; \
    case "$TIMESCALEDB_EDITION" in \
        tsl) \
            apt-get update; \
            DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
                timescaledb-toolkit-postgresql-18; \
            rm -rf /var/lib/apt/lists/* \
            ;; \
        apache) rm -f "$(pg_config --pkglibdir)"/timescaledb-tsl-*.so ;; \
        *) echo "TIMESCALEDB_EDITION must be 'tsl' or 'apache', got '$TIMESCALEDB_EDITION'" >&2; exit 1 ;; \
    esac

# timescaledb requires preloading.
RUN sed -ri "s/^[#]?\s*(shared_preload_libraries)\s*=\s*'(.*)'/\1 = 'timescaledb,pg_stat_statements,\2'/;s/,'/'/" \
    /usr/share/postgresql/postgresql.conf.sample

COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# CI overrides licenses per edition.
LABEL org.opencontainers.image.title="PostgreSQL with PostGIS and TimescaleDB" \
      org.opencontainers.image.description="PostgreSQL 18 with PostGIS 3 and TimescaleDB 2, tuned for high-throughput time-series ingest" \
      org.opencontainers.image.authors="Matej Škvarč <matej.skvarc@senlab.io>" \
      org.opencontainers.image.vendor="Matej Škvarč" \
      org.opencontainers.image.base.name="docker.io/library/postgres:18.6-trixie" \
      org.opencontainers.image.licenses="PostgreSQL AND GPL-2.0-or-later AND Apache-2.0 AND LicenseRef-Timescale"
