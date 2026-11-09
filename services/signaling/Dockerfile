# Official images, pinned by observed registry digest. TLS/operator material is
# mounted at runtime and must never enter this build context.
FROM rust:1.99.1-bookworm@sha256:e6f2cff35d0f309acf8433b4f05878ed77092602c5065cc81801177da0bb3a9c AS build
WORKDIR /work
COPY . .
RUN cargo build --locked --release -p veyra-signaling

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
LABEL org.opencontainers.image.title="Veyra signaling" \
      org.opencontainers.image.licenses="Apache-2.0"
COPY --from=build /work/target/release/veyra-signaling /usr/local/bin/veyra-signaling
COPY --from=build /work/LICENSE /usr/share/doc/veyra-signaling/LICENSE
COPY --from=build /work/docs/THIRD-PARTY-RUST.txt /usr/share/doc/veyra-signaling/THIRD-PARTY-RUST.txt
COPY --from=build /work/docs/THIRD-PARTY-NATIVE.txt /usr/share/doc/veyra-signaling/THIRD-PARTY-NATIVE.txt
COPY --from=build /work/docs/THIRD-PARTY-LICENSE-REPORT.json /usr/share/doc/veyra-signaling/THIRD-PARTY-LICENSE-REPORT.json
USER 10001:10001
EXPOSE 8443/tcp
ENTRYPOINT ["/usr/local/bin/veyra-signaling"]
