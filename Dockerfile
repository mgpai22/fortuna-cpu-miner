FROM rust:1.77.2-slim-bullseye AS build

WORKDIR /build

# leverages build cache
COPY Cargo.lock Cargo.toml ./
RUN mkdir src \
    && echo "// dummy file" > src/lib.rs \
    && cargo build --release

COPY src src
RUN cargo build --release
RUN cp ./target/release/fortuna-cpu-miner /bin/fortuna-cpu-miner

FROM debian:bullseye-slim AS final
COPY --from=build /bin/fortuna-cpu-miner /bin/
ENTRYPOINT ["/bin/fortuna-cpu-miner"]