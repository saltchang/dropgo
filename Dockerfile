# Static musl build; the vendored web assets in static/ are embedded at compile time.
FROM rust:1.95-alpine AS build
RUN apk add --no-cache build-base cmake perl
WORKDIR /src
COPY . .
RUN cargo build --release --locked && cp target/release/dropgo /dropgo && mkdir /data

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /dropgo /dropgo
# Owned by the nonroot user, so a new named volume mounted here is writable.
COPY --from=build --chown=65532:65532 /data /data
EXPOSE 8080
ENTRYPOINT ["/dropgo"]
