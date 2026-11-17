# Colophon's container image (D-172): colophon-server, the colophon CLI and the Studio, on
# distroless static, non-root. Build from the repository root:
#
#   docker build -f deploy/image/Dockerfile -t colophon:local .
#
# By default the binaries are compiled here, statically against musl (`BINS=source`). The
# release workflow passes the binaries it already built and checked against SHA256SUMS instead:
#
#   docker buildx build -f deploy/image/Dockerfile --build-arg BINS=prebuilt \
#     --build-context prebuilt=<dir with linux-amd64/ and linux-arm64/> \
#     --platform linux/amd64,linux/arm64 .
#
# Every base image is pinned by digest. To move one, change the tag, look up the new digest with
# `docker buildx imagetools inspect <image>:<tag>`, and run the quick start's CI job.

# Where the two binaries come from: `source` (compiled below) or `prebuilt` (a build context).
ARG BINS=source

# The Studio is static files, the same on every architecture: build it once, natively.
FROM --platform=$BUILDPLATFORM node:24-trixie-slim@sha256:af02d57b57719b9657a45199a79264a9437940cd55b3c7b3fc99615654f124a0 AS studio
WORKDIR /src
COPY package.json package-lock.json .npmrc ./
COPY packages ./packages
COPY apps ./apps
RUN --mount=type=cache,target=/root/.npm \
    npm ci --no-audit --no-fund --workspace @colophon/studio --include-workspace-root=false \
    && npm run build --workspace @colophon/studio \
    && test -f apps/studio/build/index.html

# Static musl binaries, as the release builds them (aws-lc's C code is compiled against musl
# too). Alpine's native target is musl, so no cross toolchain is needed.
FROM rust:1.99.1-alpine@sha256:44819b27be4c97b0e4d7203bd2e02c4820f97700211b6a37fb4c246a2de59383 AS compile
SHELL ["/bin/ash", "-eo", "pipefail", "-c"]
# The packages come from the pinned Alpine release's own repository; a version pin there breaks
# on every repository update without making the build any more reproducible than the digest.
# hadolint ignore=DL3018
RUN apk add --no-cache cmake make perl
WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY .sqlx ./.sqlx
COPY crates ./crates
# The version stamp (D-062): the commit and the build date can't come from git in here.
ARG COLOPHON_COMMIT=unknown
ARG SOURCE_DATE_EPOCH
ARG CARGO_BUILD_JOBS
ENV COLOPHON_COMMIT=$COLOPHON_COMMIT \
    SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH \
    CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS \
    CARGO_TERM_COLOR=never
# The image's compiler is the pinned one (D-013): refuse to build with anything else.
RUN channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml); \
    if ! rustc --version | grep -q "^rustc $channel "; then \
      echo "rust-toolchain.toml pins $channel but the image has $(rustc --version)" >&2; exit 1; \
    fi
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    RUSTUP_TOOLCHAIN="$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)" \
    cargo build --release --locked -p colophon-cli -p colophon-server \
    && mkdir /out \
    && cp target/release/colophon target/release/colophon-server /out/ \
    && for bin in /out/*; do \
         interpreters=$(readelf -l "$bin" | grep -c 'program interpreter' || true); \
         if [ "$interpreters" != 0 ]; then echo "$bin is not static" >&2; exit 1; fi; \
       done

FROM scratch AS bins-source
COPY --from=compile /out/ /

FROM scratch AS bins-prebuilt
ARG TARGETARCH
# `prebuilt` is a build context the release passes (--build-context), not a stage.
# hadolint ignore=DL3022
COPY --from=prebuilt linux-${TARGETARCH}/colophon linux-${TARGETARCH}/colophon-server /

# One of the two stages above, by name; nothing is pulled.
# hadolint ignore=DL3006
FROM bins-${BINS} AS bins

FROM gcr.io/distroless/static-debian13:nonroot@sha256:aafe993665bf6200db38a02c11a9351e782a84b6a45e7f33b6695638c88e3931
ARG VERSION=dev
ARG REVISION=unknown
LABEL org.opencontainers.image.title="Colophon" \
      org.opencontainers.image.description="Colophon CMS: the server, the colophon CLI and the Studio" \
      org.opencontainers.image.source="https://github.com/akira-shirase-oss/colophon-cms" \
      org.opencontainers.image.url="https://github.com/akira-shirase-oss/colophon-cms" \
      org.opencontainers.image.documentation="https://github.com/akira-shirase-oss/colophon-cms/blob/main/deploy/runbooks/install.md" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.revision="$REVISION"
COPY --from=bins --chmod=0555 /colophon /colophon-server /usr/local/bin/
COPY --from=studio /src/apps/studio/build /usr/share/colophon/studio
# Time zones are built into the binaries (jiff's bundled database) and so are the TLS roots
# (D-156): the image needs no tzdata and no CA bundle of its own.
ENV COLOPHON_STUDIO_DIR=/usr/share/colophon/studio \
    COLOPHON_LISTEN_ADDR=0.0.0.0:8080 \
    COLOPHON_PUBLIC_URL=http://localhost:8080 \
    COLOPHON_LOG_FORMAT=json \
    COLOPHON_INSTALL_KIND=container
USER 65532:65532
EXPOSE 8080
# `colophon doctor` exits 0 only when /readyz says ready; there is no shell to run anything else.
HEALTHCHECK --interval=10s --timeout=5s --start-period=60s --start-interval=1s --retries=3 \
    CMD ["/usr/local/bin/colophon", "doctor", "--server", "http://127.0.0.1:8080", "--timeout-ms", "3000"]
ENTRYPOINT ["/usr/local/bin/colophon-server"]
CMD ["--role", "all"]
