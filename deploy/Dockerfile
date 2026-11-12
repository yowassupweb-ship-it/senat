# Senate node as its own container. Build context = the unpacked source archive root.
# Debian on both stages so the binary links against the same glibc it runs on.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY ui/dist ui/dist
# one job: the server is shared with MBOX and must not run out of memory during the build
ENV CARGO_BUILD_JOBS=1
RUN cargo build --release --locked -p senate-node

FROM node:22-bookworm-slim
# What agents need to do real work: git, ripgrep, curl, and the agent CLIs themselves.
RUN apt-get update \
 && apt-get install -y --no-install-recommends git curl ca-certificates ripgrep openssh-client \
 && rm -rf /var/lib/apt/lists/* \
 && npm install -g @anthropic-ai/claude-code @openai/codex @moonshot-ai/kimi-code \
 && npm cache clean --force
COPY --from=build /src/target/release/senate-node /usr/local/bin/senate-node
RUN useradd --create-home --home-dir /data --uid 1001 senate \
 && mkdir -p /workspace && chown senate:senate /data /workspace
USER senate
ENV SENATE_HOME=/data HOME=/data PATH=/data/.local/bin:/usr/local/bin:/usr/bin:/bin
WORKDIR /workspace
VOLUME ["/data", "/workspace"]
EXPOSE 7777
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD curl -fsS http://127.0.0.1:7777/api/health || exit 1
CMD ["senate-node", "serve", "--bind", "0.0.0.0:7777"]
