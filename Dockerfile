FROM lukemathwalker/cargo-chef:latest AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin timothe-rs

FROM debian:trixie-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y openssl ca-certificates

COPY --from=builder /app/target/release/timothe-rs /usr/local/bin
ENTRYPOINT ["/usr/local/bin/timothe-rs"]