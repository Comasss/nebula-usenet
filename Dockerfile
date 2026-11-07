# syntax=docker/dockerfile:1.7
# nebula-server en un contenedor: la interfaz web, las APIs Newznab/SABnzbd y el motor.
#   docker build -t nebula .
#   docker run -d -p 5080:5080 -v ./config:/config -v ./downloads:/downloads nebula
# Convenciones de linuxserver.io: PUID, PGID, UMASK y TZ; datos en /config y descargas en /downloads.
# Se compila todo aquí (interfaz, servidor estático con musl, par2cmdline-turbo y 7-Zip), así que
# funciona igual en amd64 y arm64. Más detalles en docs/DOCKER.md.

ARG BASE_IMAGE=debian:trixie-slim
ARG RUST_IMAGE=rust:1-slim-trixie
ARG NODE_IMAGE=node:22-trixie-slim
ARG PAR2_VERSION=v1.5.0
ARG SEVENZIP_VERSION=25.01

# ---- Interfaz (Svelte → dist/) ----
FROM ${NODE_IMAGE} AS ui
WORKDIR /src
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY index.html vite.config.js ./
COPY src src
RUN npm run build

# ---- nebula-server (estático, con la interfaz dentro) y la CLI ----
FROM ${RUST_IMAGE} AS server
RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools perl make \
 && rm -rf /var/lib/apt/lists/*
RUN rustup target add "$(uname -m)-unknown-linux-musl"
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# El workspace incluye la app de escritorio: basta con su manifiesto y su código para que Cargo
# lo cargue; no se compila.
COPY src-tauri/Cargo.toml src-tauri/build.rs src-tauri/
COPY src-tauri/src src-tauri/src
# Miembro del workspace (el servidor NNTP de dev/bench.py): hace falta para que Cargo cargue el workspace.
COPY dev/nntp-bench dev/nntp-bench
COPY --from=ui /src/dist dist
ARG TARGETARCH
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=nebula-target-${TARGETARCH},target=/src/target \
    target="$(uname -m)-unknown-linux-musl" \
 && cargo build --release --locked -p nebula-server -p nebula-cli --target "$target" \
 && install -Dm755 "target/$target/release/nebula-server" /out/nebula-server \
 && install -Dm755 "target/$target/release/nebula-cli" /out/nebula

# Solo los binarios: `docker buildx build --target server-bin --output type=local,dest=out .`
FROM scratch AS server-bin
COPY --from=server /out/nebula-server /out/nebula /

# ---- par2cmdline-turbo y 7-Zip (con RAR) ----
FROM ${BASE_IMAGE} AS tools
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential automake autoconf \
 && rm -rf /var/lib/apt/lists/*
ARG PAR2_VERSION
ARG SEVENZIP_VERSION
ADD https://github.com/animetosho/par2cmdline-turbo.git#${PAR2_VERSION} /build/par2
ADD https://github.com/ip7z/7zip.git#${SEVENZIP_VERSION} /build/7zip
RUN cd /build/par2 \
 && ./automake.sh \
 && ./configure \
 && make -j"$(nproc)" \
 && install -Dm755 par2 /out/par2
RUN cd /build/7zip/CPP/7zip/Bundles/Alone2 \
 && make -j"$(nproc)" -f ../../cmpl_gcc.mak \
 && install -Dm755 "$(find . -type f -name 7zz | head -1)" /out/7zz
RUN strip /out/par2 /out/7zz && /out/7zz i | head -3 && /out/par2 -V

# ---- Imagen final ----
FROM ${BASE_IMAGE}
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata libstdc++6 libgomp1 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=tools /out/par2 /out/7zz /usr/local/bin/
# nebula-server y la CLI: `docker exec nebula nebula status`
COPY --from=server /out/nebula-server /out/nebula /usr/local/bin/
COPY deploy/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
ENV NEBULA_DOCKER=1 \
    NEBULA_DATA_DIR=/config \
    NEBULA_DOWNLOAD_DIR=/downloads \
    NEBULA_PORT=5080 \
    PUID=1000 \
    PGID=1000 \
    UMASK=022 \
    TZ=Etc/UTC \
    HOME=/config
VOLUME ["/config", "/downloads"]
EXPOSE 5080
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD ["nebula-server", "healthcheck"]
ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["nebula-server"]
LABEL org.opencontainers.image.title="Nebula" \
      org.opencontainers.image.description="Nebula Usenet downloader: web interface, Newznab and SABnzbd APIs" \
      org.opencontainers.image.source="https://github.com/Comasss/nebula-usenet" \
      org.opencontainers.image.licenses="PolyForm-Noncommercial-1.0.0"
