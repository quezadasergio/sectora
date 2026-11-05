#!/usr/bin/env bash
# Compila GNU ddrescue como sidecar Windows (PE32+) con MinGW-w64.
# GNU ddrescue asume POSIX (termios/fsync/sigaction); MinGW no las tiene,
# así que inyectamos shims en scripts/ddrescue-mingw/.
#
# Uso:
#   bash scripts/build-ddrescue-windows.sh
#   # o con compilador cruzado en Linux:
#   CXX=x86_64-w64-mingw32-g++ bash scripts/build-ddrescue-windows.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMPAT="$ROOT/scripts/ddrescue-mingw"
OUT_DIR="$ROOT/src-tauri/binaries"
DEST="$OUT_DIR/ddrescue-x86_64-pc-windows-msvc.exe"
VERSION="${DDRESCUE_VERSION:-1.29}"
JOBS="${JOBS:-$(command -v nproc >/dev/null && nproc || echo 2)}"

if [[ ! -f "$COMPAT/mingw_posix.h" || ! -f "$COMPAT/compat/termios.h" ]]; then
  echo "ERROR: faltan shims MinGW en $COMPAT" >&2
  exit 1
fi

CXX_BIN="${CXX:-}"
if [[ -z "$CXX_BIN" ]]; then
  if command -v x86_64-w64-mingw32-g++ >/dev/null 2>&1; then
    CXX_BIN=x86_64-w64-mingw32-g++
  elif command -v g++ >/dev/null 2>&1; then
    CXX_BIN=g++
  else
    echo "ERROR: no hay g++ / x86_64-w64-mingw32-g++" >&2
    exit 127
  fi
fi

echo "==> Building GNU ddrescue ${VERSION} for Windows"
echo "    CXX=$CXX_BIN"
echo "    DEST=$DEST"
"$CXX_BIN" --version | head -1

BUILD_ROOT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
BUILD_ROOT="${BUILD_ROOT%/}/sectora-ddrescue-win"
rm -rf "$BUILD_ROOT"
mkdir -p "$BUILD_ROOT"
cd "$BUILD_ROOT"

TARBALL="ddrescue-${VERSION}.tar.lz"
URL="https://ftp.gnu.org/gnu/ddrescue/${TARBALL}"
echo "==> Descargando $URL"
if command -v wget >/dev/null 2>&1; then
  wget -q "$URL"
else
  curl -fsSL -o "$TARBALL" "$URL"
fi

if command -v lzip >/dev/null 2>&1; then
  lzip -cd "$TARBALL" | tar -xf -
else
  tar --lzip -xf "$TARBALL"
fi
cd "ddrescue-${VERSION}"

# Cabeceras de compat en el árbol de build (include paths relativos estables).
cp -R "$COMPAT/compat" ./compat
cp "$COMPAT/mingw_posix.h" ./mingw_posix.h

./configure \
  "CXX=${CXX_BIN}" \
  "CPPFLAGS=-I$(pwd)/compat -include $(pwd)/mingw_posix.h" \
  "CXXFLAGS=-Wall -W -O2 -static-libgcc -static-libstdc++" \
  "LDFLAGS=-static-libgcc -static-libstdc++"

make -j"$JOBS" V=1

SRC=""
for cand in ddrescue.exe ddrescue; do
  if [[ -f "$cand" ]]; then SRC="$cand"; break; fi
done
if [[ -z "$SRC" ]]; then
  echo "ERROR: no se generó el binario ddrescue" >&2
  ls -la
  exit 1
fi

mkdir -p "$OUT_DIR"
cp -f "$SRC" "$DEST"
chmod +x "$DEST" 2>/dev/null || true
ls -lh "$DEST"
echo "OK Windows ddrescue sidecar listo"
