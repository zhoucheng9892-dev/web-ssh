# ---- Stage 1: build the frontend (Vue 3 + Vite) ----
FROM node:20-bookworm-slim AS frontend-build
WORKDIR /app/frontend

# Install deps first (cached unless package files change).
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci

# Build static assets into /app/frontend/dist.
COPY frontend/ ./
RUN npm run build

# ---- Stage 2: build the backend (Rust), embedding the frontend ----
FROM rust:1-bookworm AS backend-build
WORKDIR /app/backend

# Pre-fetch deps for better layer caching. Copy only manifests first.
COPY backend/Cargo.toml backend/Cargo.lock ./
# Create a stub so `cargo fetch` resolves deps without compiling our crate.
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo fetch && rm -rf src

# Now copy the real sources and the built frontend (rust-embed reads ../frontend/dist).
COPY backend/ ./
COPY --from=frontend-build /app/frontend/dist ../frontend/dist
# Force a rebuild now that real sources are in place.
RUN touch src/main.rs && cargo build --release

# ---- Stage 3: minimal runtime image ----
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# The binary embeds the frontend, so the image needs no static files.
# ca-certificates: for russh/TLS to remote hosts; tini: proper signal handling.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates tini \
        openssh-client \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for safety. Data dir is /app/data (mounted as a volume).
RUN useradd --create-home --uid 1000 --shell /usr/sbin/nologin webssh \
    && mkdir -p /app/data && chown -R webssh:webssh /app
COPY --from=backend-build /app/backend/target/release/web-ssh-backend /usr/local/bin/web-ssh-backend

USER webssh
ENV WEBSSH_DATA_DIR=/app/data \
    WEBSSH_HOST=0.0.0.0 \
    WEBSSH_PORT=3000
EXPOSE 3000

# tini reaps zombies and forwards signals so the server shuts down cleanly.
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["web-ssh-backend"]
