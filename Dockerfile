FROM rust:1.94-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY web/ ./web/
RUN cargo build --release -p twentytoo --example demo

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/examples/demo /usr/local/bin/twentytoo-demo
ENV ADDR=0.0.0.0:3000
EXPOSE 3000
CMD ["twentytoo-demo"]
