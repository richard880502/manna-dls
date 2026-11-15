# syntax=docker/dockerfile:1.7
# Single image: React static build served by the Rust API.

# ---------- frontend ----------
FROM node:22-bookworm-slim AS web
WORKDIR /src/web
RUN corepack enable && corepack prepare pnpm@11.20.0 --activate
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# ---------- backend ----------
FROM rust:1-bookworm AS api
WORKDIR /src/api
COPY api/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/api/target \
    cargo build --release --locked \
 && cp target/release/manna-api /usr/local/bin/manna-api

# ---------- runtime ----------
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates fonts-wqy-microhei \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 app \
 && mkdir -p /app/data/materials \
 && chown -R app /app/data
WORKDIR /app
COPY --from=api /usr/local/bin/manna-api /app/manna-api
COPY --from=web /src/web/dist /app/web-dist
# fonts-wqy-microhei: Chinese glyphs for worksheet PDFs (teacher notes); Latin fonts are built in.
ENV STATIC_DIR=/app/web-dist \
    WORKSHEET_CJK_FONT=/usr/share/fonts/truetype/wqy/wqy-microhei.ttc \
    PORT=8080 \
    RUST_LOG=info
USER app
EXPOSE 8080
CMD ["/app/manna-api"]
