#!/usr/bin/env bash
# Builds tools/gll2wavey inside a gll-tools checkout and runs it.
#   scripts/gll2wavey.sh -in speaker.gll -out speaker.waveyspk [-filters k1,k2,…]
#   scripts/gll2wavey.sh -in speaker.gll -list
# Requires Go and git. gll-tools: https://github.com/CWBudde/gll-tools (MIT).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/target.nosync/gll"
mkdir -p "$WORK"
clone() { [[ -d "$WORK/$2" ]] || git clone -q "https://github.com/$1.git" "$WORK/$2"; }
clone CWBudde/gll-tools gll-tools
clone cwbudde/go-sofa go-sofa
clone cwbudde/go-hdf5 go-hdf
mkdir -p "$WORK/gll-tools/cmd/gll2wavey"
cp "$ROOT/tools/gll2wavey/main.go" "$WORK/gll-tools/cmd/gll2wavey/main.go"
(cd "$WORK/gll-tools" && go mod tidy >/dev/null 2>&1 && go build -o "$WORK/gll2wavey" ./cmd/gll2wavey)
exec "$WORK/gll2wavey" "$@"
