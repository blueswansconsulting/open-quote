# syntax=docker/dockerfile:1

# ---- build ----
FROM rust:1-slim-bookworm AS build
WORKDIR /src

# Cache dependencies separately from the app source.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
COPY static ./static
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

# ---- runtime: no shell, no package manager, non-root ----
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/open-quote /usr/local/bin/open-quote
ENV HOST=0.0.0.0 \
    PORT=8790
EXPOSE 8790
USER nonroot:nonroot
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/open-quote", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/open-quote"]
