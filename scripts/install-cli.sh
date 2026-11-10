#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$1"
[[ -f "$ROOT/vsmedia" ]] || exit 1
BIN="$HOME/.local/bin"
mkdir -p -- "$BIN"
ln -sfn -- "$ROOT/vsmedia" "$BIN/vsmedia"
chmod u+x "$ROOT/vsmedia"
echo "[VSMedia] Comando instalado: $BIN/vsmedia"
case ":$PATH:" in
  *":$BIN:"*) echo 'Ejecutá: vsmedia web' ;;
  *) echo "Agregá $BIN a tu PATH y abrí una terminal nueva." ;;
esac
