FROM rust:bookworm AS builder
RUN apt update && apt install -y \
  pkg-config \
  libssl-dev \ 
  libdbus-1-dev \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY ./Cargo.toml .
COPY ./Cargo.lock .
RUN mkdir -p ./src
RUN echo "fn main() {println!(\"if you see this, the build broke\")}" > ./src/main.rs
RUN cargo build --release
RUN rm -f target/release/deps/$(cat Cargo.toml | awk '/name/ {print}' | cut -d '"' -f 2 | sed 's/-/_/')*

COPY . .
RUN cargo build --release
RUN cp -r target/release/$(cat Cargo.toml | awk '/name/ {print}' | cut -d '"' -f 2) /app/dummy-wca

FROM debian:bookworm-slim
WORKDIR /app

RUN apt update && apt install -y \
  openssl \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/dummy-wca /app/dummy-wca
COPY --from=builder /app/templates /app/templates
COPY --from=builder /app/data /app/data

ENTRYPOINT ["/app/dummy-wca"]