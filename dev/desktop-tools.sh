#!/usr/bin/env bash
# Compila par2cmdline-turbo y 7-Zip para la app de escritorio de Linux o macOS y los deja en
# src-tauri/bin con el nombre que espera Tauri para los "sidecars" (nebula-par2-<triple>,
# nebula-7zz-<triple>). `tauri build --config src-tauri/tauri.tools.conf.json` los mete en el
# paquete, junto al ejecutable de Nebula, que los busca ahí antes que en el PATH.
#
#   dev/desktop-tools.sh                         # Linux, la arquitectura de esta máquina
#   dev/desktop-tools.sh universal-apple-darwin  # macOS, x86_64 + arm64 en un solo binario
#
# Mismas versiones que la imagen de Docker (Dockerfile).
set -euo pipefail

PAR2_VERSION=${PAR2_VERSION:-v1.5.0}
SEVENZIP_VERSION=${SEVENZIP_VERSION:-25.01}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/src-tauri/bin"
WORK=${WORK:-"$ROOT/target/desktop-tools"}
TRIPLE=${1:-$(rustc -vV | sed -n 's/^host: //p')}
JOBS=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)

mkdir -p "$WORK" "$OUT"
cd "$WORK"
[ -d par2 ] || git clone --depth 1 --branch "$PAR2_VERSION" https://github.com/animetosho/par2cmdline-turbo.git par2
[ -d 7zip ] || git clone --depth 1 --branch "$SEVENZIP_VERSION" https://github.com/ip7z/7zip.git 7zip

# par2 para una arquitectura: build_par2 <carpeta> [opciones de configure]
build_par2() {
  local dir=$1; shift
  rm -rf "$dir" && cp -R par2 "$dir"
  (cd "$dir" && ./automake.sh >/dev/null && ./configure "$@" >/dev/null && make -j"$JOBS" >/dev/null)
}

# 7zz para una plataforma: build_7zz <makefile de CPP/7zip>
build_7zz() {
  rm -rf 7zip/CPP/7zip/Bundles/Alone2/b
  (cd 7zip/CPP/7zip/Bundles/Alone2 && make -j"$JOBS" -f "../../$1" >/dev/null)
  find 7zip/CPP/7zip/Bundles/Alone2 -type f -name 7zz -perm -u+x | head -1
}

case "$TRIPLE" in
  *-linux-*)
    build_par2 par2-build
    install -m755 par2-build/par2 "$OUT/nebula-par2-$TRIPLE"
    install -m755 "$(build_7zz cmpl_gcc.mak)" "$OUT/nebula-7zz-$TRIPLE"
    strip "$OUT/nebula-par2-$TRIPLE" "$OUT/nebula-7zz-$TRIPLE"
    ;;
  universal-apple-darwin)
    # Cada arquitectura por separado (par2 elige las instrucciones SIMD según la arquitectura) y
    # luego un binario universal con lipo.
    build_par2 par2-arm64 --host=aarch64-apple-darwin CC="clang -arch arm64" CXX="clang++ -arch arm64"
    build_par2 par2-x64 --host=x86_64-apple-darwin CC="clang -arch x86_64" CXX="clang++ -arch x86_64"
    lipo -create par2-arm64/par2 par2-x64/par2 -output "$OUT/nebula-par2-$TRIPLE"
    arm=$(build_7zz cmpl_mac_arm64.mak); cp "$arm" 7zz-arm64
    x64=$(build_7zz cmpl_mac_x64.mak); cp "$x64" 7zz-x64
    lipo -create 7zz-arm64 7zz-x64 -output "$OUT/nebula-7zz-$TRIPLE"
    chmod 755 "$OUT/nebula-par2-$TRIPLE" "$OUT/nebula-7zz-$TRIPLE"
    lipo -info "$OUT/nebula-par2-$TRIPLE" "$OUT/nebula-7zz-$TRIPLE"
    ;;
  *)
    echo "Plataforma no soportada: $TRIPLE" >&2
    exit 1
    ;;
esac

"$OUT/nebula-par2-$TRIPLE" -V | head -1
"$OUT/nebula-7zz-$TRIPLE" i | head -3
ls -la "$OUT"/nebula-*-"$TRIPLE"
