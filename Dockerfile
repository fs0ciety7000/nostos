# nostos-api image for Coolify. Build from the repository root:
#   docker build -t nostos-api .
FROM rust:1.89-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p nostos-api --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 nostos
COPY --from=build /src/target/release/nostos-api /usr/local/bin/nostos-api
USER nostos
ENV NOSTOS_BIND=0.0.0.0:8080
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s CMD curl -fsS http://localhost:8080/health || exit 1
CMD ["nostos-api"]
