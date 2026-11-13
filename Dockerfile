# ---- 1. Build the PWA ----
FROM node:22-alpine AS frontend
WORKDIR /src
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ---- 2. Build the Rust server ----
FROM rust:1-bookworm AS backend
WORKDIR /src
# Cache dependencies: build once with a dummy main, then with the real code.
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY backend/ ./
RUN touch src/main.rs && cargo build --release

# ---- 3. Small runtime image ----
FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home /app cattracker \
 && mkdir -p /data && chown cattracker /data
WORKDIR /app
COPY --from=backend /src/target/release/cattracker /usr/local/bin/cattracker
COPY --from=frontend /src/dist /app/static
ENV CT_DB_PATH=/data/cattracker.db \
    CT_STATIC_DIR=/app/static \
    CT_BIND=0.0.0.0:8080 \
    CT_COOKIE_SECURE=true \
    RUST_LOG=cattracker=info,tower_http=warn
USER cattracker
VOLUME /data
EXPOSE 8080
CMD ["cattracker", "serve"]
