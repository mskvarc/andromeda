# syntax=docker/dockerfile:1

# PostgreSQL 18 on Debian trixie with PostGIS and TimescaleDB from the official
# PGDG and Timescale apt repositories. Building the extensions from source is
# unnecessary on Debian and would leave a full C/LLVM toolchain in the final
# image; the packaged builds are smaller, signed, and track upstream security
# fixes. timescaledb-tune and timescaledb-parallel-copy ship in timescaledb-tools,
# so no separate Go build stage is needed either.
FROM postgres:18.6-trixie

# The PGDG repository (PostGIS) is already configured in the base image; add the
# Timescale repository and its signing key beside it. os-release supplies the
# codename so the list stays correct if the base image moves to a newer Debian.
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

# PostGIS from PGDG; the TimescaleDB extension, its loader, and the tuning/copy
# tools from Timescale. pg_stat_statements (contrib) already ships in the base.
RUN set -eux; \
    apt-get update; \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        postgresql-18-postgis-3 \
        postgresql-18-postgis-3-scripts \
        timescaledb-2-postgresql-18 \
        timescaledb-2-loader-postgresql-18 \
        timescaledb-tools; \
    rm -rf /var/lib/apt/lists/*

# TimescaleDB edition, matching the two builds the source Dockerfile supported:
#   tsl    - Timescale Community Edition (default): the Apache-2.0 core plus the
#            TSL-licensed capabilities (hypercore/compression, continuous
#            aggregates, retention and reorder policies, background job
#            scheduling, SkipScan). Also carries the timescaledb-toolkit
#            hyperfunctions, which are themselves TSL-licensed. Free for
#            self-managed use; forbids offering the software as a hosted service.
#   apache - Apache-2.0 only: hypertables, time_bucket, core hyperfunctions.
# The apt package ships the Apache module (timescaledb-<ver>.so) and the TSL
# module (timescaledb-tsl-<ver>.so) side by side. The apache edition removes the
# TSL module so no TSL-licensed binary remains, which is the apt equivalent of
# the source build's -DAPACHE_ONLY=1, and installs no toolkit for the same
# licensing reason. The loader is edition-independent. The toolkit is a plain
# extension (CREATE EXTENSION timescaledb_toolkit); unlike the base extension it
# needs no shared_preload_libraries entry.
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

# timescaledb must be loaded via shared_preload_libraries to function at all;
# pg_stat_statements backs the broker's query-performance diagnostics. The
# Debian image exposes the sample config at this fixed, version-agnostic path.
RUN sed -ri "s/^[#]?\s*(shared_preload_libraries)\s*=\s*'(.*)'/\1 = 'timescaledb,pg_stat_statements,\2'/;s/,'/'/" \
    /usr/share/postgresql/postgresql.conf.sample

# Custom entrypoint: the upstream postgres bootstrap plus a timescaledb-tune pass
# and the Andromeda postgresql.conf overrides applied on every start. Overriding
# the file at this path keeps the base image's ENTRYPOINT/CMD/EXPOSE/PGDATA.
COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# OCI image annotations: https://specs.opencontainers.org/image-spec/annotations/
#
# org.opencontainers.image.licenses is the SPDX expression for the software the
# image *contains* -- not this Dockerfile's own licence and not any consumer's.
# The image ships PostgreSQL, PostGIS, and TimescaleDB, so the expression covers
# those. The default below is the tsl (Community) edition, which carries the
# TSL-licensed TimescaleDB module and toolkit; the apache edition removes both,
# narrowing the expression to "PostgreSQL AND GPL-2.0-or-later AND Apache-2.0".
# The build pipeline stamps the correct per-edition value; this default is right
# for a local tsl build. TSL has no SPDX identifier, hence LicenseRef-Timescale.
#
# Dynamic annotations (created, revision, version, source, url) are stamped by
# the build pipeline, which knows the commit and repository, and are not
# hardcoded here. authors is a neutral placeholder: replace it with a real
# maintainer identity if you want personal attribution published.
LABEL org.opencontainers.image.title="PostgreSQL with PostGIS and TimescaleDB" \
      org.opencontainers.image.description="PostgreSQL 18 with PostGIS 3 and TimescaleDB 2, tuned for high-throughput time-series ingest" \
      org.opencontainers.image.authors="Matej Škvarč <matej.skvarc@senlab.io>" \
      org.opencontainers.image.vendor="Matej Škvarč" \
      org.opencontainers.image.base.name="docker.io/library/postgres:18.6-trixie" \
      org.opencontainers.image.licenses="PostgreSQL AND GPL-2.0-or-later AND Apache-2.0 AND LicenseRef-Timescale"
