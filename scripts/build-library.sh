#!/usr/bin/env bash
# Builds the stock speaker library from manufacturer GLL files:
#   <library>/Speakers/<Manufacturer>/<id>.waveyspk   (converted models)
#   <library>/GLL/<Manufacturer>/<file>.gll           (the original files)
# The library defaults to ~/Documents/Wavey Library (the app's default
# stock location); pass another folder as the first argument. GLL files are
# looked up in $GLL_DIR (default ~/Downloads). Run from the repository.
#
#   scripts/build-library.sh ["/path/to/library"]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB="${1:-$HOME/Documents/Wavey Library}"
GLL_DIR="${GLL_DIR:-$HOME/Downloads}"
CONV="$ROOT/target.nosync/gll/gll2wavey"
[[ -x "$CONV" ]] || "$ROOT/scripts/gll2wavey.sh" -h >/dev/null 2>&1 || true
[[ -x "$CONV" ]] || { echo "gll2wavey not built (run scripts/gll2wavey.sh once)"; exit 1; }

mkdir -p "$LIB/Speakers" "$LIB/GLL"

# file | id | manufacturer | name | filter groups (one per box source, comma separated) | extra options
MANIFEST=$(cat <<'EOF'
Void_Acoustics_Arcline_8_v20a.gll|void-acoustics-arcline-8|Void Acoustics|Void Acoustics Arcline 8|keyFilter_HF,keyFilter_MF,keyFilter_LF
Air_Array_v2.3.gll|air-array|Void Acoustics|Void Acoustics Air Array|HF Preset,MF Preset,LF Preset,LF Preset,MF Preset,HF Preset
Air_Motion_V2_v2.3.gll|air-motion-v2|Void Acoustics|Void Acoustics Air Motion V2|HF High Pass,LF Low Pass
Incubus_Sub_v2.0.gll|incubus-sub|Void Acoustics|Void Acoustics Incubus Sub|Preset
ZuL1cbVsGrYSvTJh_Funktion-OneEvo2v1.0.gll|funktion-one-evo2|Funktion-One|Funktion-One Evo 2|keyFilterGrp3,keyFilterGrp2,keyFilterGrp1
Yamaha-DXS_GLL/Yamaha_DXS12_v2.gll|yamaha-dxs12|Yamaha|Yamaha DXS12|
Yamaha-DXS_GLL/Yamaha_DXS15_v2.gll|yamaha-dxs15|Yamaha|Yamaha DXS15|
Yamaha-DXS_GLL/Yamaha_DXS18_v2.gll|yamaha-dxs18|Yamaha|Yamaha DXS18|
Yamaha-DXSXLF_GLL/Yamaha_DXS15XLF_v2.0.gll|yamaha-dxs15xlf|Yamaha|Yamaha DXS15XLF|
Yamaha-DXSXLF_GLL/Yamaha_DXS18XLF_v2.0.gll|yamaha-dxs18xlf|Yamaha|Yamaha DXS18XLF|
Control_23-1_25-1_28-1_EASE_Data/Control_23-1.gll|jbl-control-23-1|JBL Professional|JBL Control 23-1||-origin sources
Control_23-1_25-1_28-1_EASE_Data/Control_25-1.gll|jbl-control-25-1|JBL Professional|JBL Control 25-1||-origin sources
Control_23-1_25-1_28-1_EASE_Data/Control_28-1.gll|jbl-control-28-1|JBL Professional|JBL Control 28-1||-origin sources
EOF
)

ok=0; missing=0
while IFS='|' read -r file id maker name filters extra; do
  [[ -z "$file" ]] && continue
  src="$GLL_DIR/$file"
  if [[ ! -f "$src" ]]; then echo "missing: $src"; missing=$((missing + 1)); continue; fi
  mkdir -p "$LIB/Speakers/$maker" "$LIB/GLL/$maker"
  cp -p "$src" "$LIB/GLL/$maker/$(basename "$file")"
  args=(-in "$src" -out "$LIB/Speakers/$maker/$id.waveyspk" -id "$id" -name "$name" -manufacturer "$maker")
  [[ -n "$filters" ]] && args+=(-filters "$filters")
  # shellcheck disable=SC2206
  [[ -n "${extra:-}" ]] && args+=($extra)
  echo "== $name"
  nice -n 15 "$CONV" "${args[@]}" 2>&1 | grep -vE "^warning|^  [−-]6 dB" | sed 's/^/   /'
  ok=$((ok + 1))
done <<< "$MANIFEST"
echo "library: $LIB ($ok converted, $missing missing)"
