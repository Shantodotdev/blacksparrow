# Black Sparrow HTTP API (Firecrawl-compatible) for self-hosting.
#
#   docker build -t blacksparrow .
#   docker run -p 3002:3002 -e BLACKSPARROW_API_KEYS=change-me -v bs-data:/data blacksparrow
#
# Pages that need JavaScript are rendered by a separate headless Chrome (see
# docker-compose.yml); without one, scraping still works for server-rendered pages.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p blacksparrow --features serve \
    && cp target/release/blacksparrow /usr/local/bin/blacksparrow

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin blacksparrow \
    && mkdir -p /data \
    && chown blacksparrow /data
COPY --from=build /usr/local/bin/blacksparrow /usr/local/bin/blacksparrow
USER blacksparrow
ENV BLACKSPARROW_DB_PATH=/data/blacksparrow.db
VOLUME /data
EXPOSE 3002
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD curl -fsS http://127.0.0.1:3002/health || exit 1
ENTRYPOINT ["blacksparrow"]
CMD ["serve", "--host", "0.0.0.0", "--port", "3002"]
