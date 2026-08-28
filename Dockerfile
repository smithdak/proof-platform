FROM rust:1-slim-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --workspace

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 proof \
    && useradd --uid 10001 --gid proof --create-home --shell /usr/sbin/nologin proof

COPY --from=builder /build/target/release/proof-transport-http /usr/local/bin/proof-transport-http
COPY --from=builder /build/target/release/proof-transport-cli /usr/local/bin/proof-transport-cli

ENV PROOF_WORKSPACE=/data \
    RUST_LOG=info

RUN mkdir -p /data/.proof/data/proofs /data/.proof/registry /data/.proof/storage \
    && chown -R proof:proof /data

USER proof
WORKDIR /data

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:3000/health"]

ENTRYPOINT ["/usr/local/bin/proof-transport-http"]
