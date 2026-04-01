FROM rust:1.88-slim AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependencies
COPY Cargo.toml ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release
RUN rm -f target/release/deps/dockup_api*

# Build real binary
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

FROM debian:bookworm-slim AS agent-downloader
RUN apt-get update && apt-get install -y curl ca-certificates && rm -rf /var/lib/apt/lists/*
ARG AGENT_VERSION=0.1.9
RUN mkdir -p /agent-binaries && \
    curl -fsSL -o /agent-binaries/dockup-agent-x86_64 \
      "https://github.com/doublej0/dockup/releases/download/agent-v${AGENT_VERSION}/dockup-agent-x86_64" && \
    curl -fsSL -o /agent-binaries/dockup-agent-aarch64 \
      "https://github.com/doublej0/dockup/releases/download/agent-v${AGENT_VERSION}/dockup-agent-aarch64" && \
    chmod +x /agent-binaries/*

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -g 1000 appuser && useradd -u 1000 -g appuser -s /bin/sh appuser

RUN mkdir -p /data && chown appuser:appuser /data

WORKDIR /app

COPY --from=builder /app/target/release/dockup-api ./dockup-api
COPY --from=builder /app/migrations ./migrations
COPY --from=agent-downloader /agent-binaries /app/agent-binaries

RUN chown -R appuser:appuser /app

USER appuser

EXPOSE 3101

ENTRYPOINT ["./dockup-api"]
