#!/usr/bin/env bash
# Prepara el sidecar GNU ddrescue para el target del host actual.
# En CI: REQUIRE_DDRESCUE=1 hace fallar si no hay binario real.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/src-tauri/binaries"
mkdir -p "$OUT"

REQUIRE="${REQUIRE_DDRESCUE:-0}"

echo "==> Sectora: preparando sidecars de ddrescue en $OUT"

is_real_binary() {
  local f="$1"
  [[ -f "$f" ]] || return 1
  if command -v file >/dev/null 2>&1; then
    file "$f" | grep -qiE 'executable|Mach-O|ELF|PE32|MS-DOS'
  else
    [[ -x "$f" ]] || [[ "$f" == *.exe ]]
  fi
}

install_one() {
  local triple="$1"
  local dest="$2"
  local src="${3:-}"

  if [[ -n "$src" && -f "$src" ]]; then
    cp -f "$src" "$dest"
    chmod +x "$dest" 2>/dev/null || true
    echo "OK  ${triple} <- ${src}"
    return 0
  fi

  if is_real_binary "$dest"; then
    echo "OK  ${triple} (ya presente)"
    return 0
  fi

  echo "MISSING ${triple}: se esperaba un binario en ${dest}"
  echo "        Fuentes: https://www.gnu.org/software/ddrescue/  (GPLv2+)"
  if [[ "$REQUIRE" == "1" ]]; then
    return 1
  fi
  return 0
}

OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
  Darwin-arm64)
    SRC=""
    if command -v ddrescue >/dev/null 2>&1; then
      SRC="$(command -v ddrescue)"
    elif [[ -x /opt/homebrew/bin/ddrescue ]]; then
      SRC=/opt/homebrew/bin/ddrescue
    fi
    install_one aarch64-apple-darwin "$OUT/ddrescue-aarch64-apple-darwin" "$SRC"
    ;;
  Linux-x86_64)
    SRC=""
    if command -v ddrescue >/dev/null 2>&1; then
      SRC="$(command -v ddrescue)"
    elif [[ -x /usr/bin/ddrescue ]]; then
      SRC=/usr/bin/ddrescue
    fi
    install_one x86_64-unknown-linux-gnu "$OUT/ddrescue-x86_64-unknown-linux-gnu" "$SRC"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    echo "En Windows usa el paso CI de MSYS2 o coloca"
    echo "  $OUT/ddrescue-x86_64-pc-windows-msvc.exe"
    if [[ "$REQUIRE" == "1" ]]; then
      install_one x86_64-pc-windows-msvc "$OUT/ddrescue-x86_64-pc-windows-msvc.exe" ""
    fi
    ;;
  *)
    echo "Host ${OS}/${ARCH}: no se genera sidecar nativo en este script."
    if [[ "$REQUIRE" == "1" ]]; then
      exit 1
    fi
    ;;
esac

echo
echo "Recuerda: GNU ddrescue es GPLv2+. Ver THIRD_PARTY_DDRESCUE.md"
echo "macOS de Sectora: solo Apple Silicon (aarch64-apple-darwin)."
