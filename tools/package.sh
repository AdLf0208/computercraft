#!/usr/bin/env bash
# Package both delivered artifacts from the current sources:
#  - the source-only `.hnmod` in the port root (`tools/package.py`, deterministic)
#  - the `.hmod` in `content/`, built and checked by the pin's own
#    `cargo-horizon-mod` (the same code the registry runs).
# Run inside a build slot:
#   ports/tools/build-slot.sh run --job <id>-pack --port ports/<id> \
#       --step package -- env CARGO_INCREMENTAL=0 /abs/path/tools/package.sh
set -euo pipefail

PORT=$(cd "$(dirname "$0")/.." && pwd)
HORIZON=${HORIZON:-${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}/ports/_shared/pins/horizon-1d63e0a}
ID=$(python3 -c 'import sys,tomllib;print(tomllib.load(open(sys.argv[1],"rb"))["id"])' \
        "$PORT/content/horizon-mod.toml")
LOGS=$PORT/target/ladder
mkdir -p "$LOGS"
cd "$PORT"

# Use the pin's prebuilt packaging tool when it is there (no 4-minute build).
if [ -x "$HORIZON/target/debug/cargo-horizon-mod" ]; then
    hm() { (cd content && "$HORIZON/target/debug/cargo-horizon-mod" horizon-mod "$@"); }
else
    CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PORT/target/carry-tools}
    export CARGO_TARGET_DIR
    hm() { (cd content && cargo run --locked --offline --quiet \
        --manifest-path "$HORIZON/crates/cargo-horizon-mod/Cargo.toml" -- horizon-mod "$@"); }
fi

python3 tools/package.py | tee "$LOGS/package-hnmod.log"
cp target/"$ID"-*.hnmod "$ID.hnmod"

hm build 2>&1 | tee "$LOGS/package-hmod.log"
cp "content/target/horizon-mod/$ID.hmod" "content/$ID.hmod"
hm check "$ID.hmod" 2>&1 | tee -a "$LOGS/package-hmod.log"

echo "== artifacts =="
for f in "content/$ID.hmod" "$ID.hnmod"; do
    [ -f "$f" ] || { echo "missing $f"; exit 1; }
    size=$(stat -c %s "$f")
    sha=$(sha256sum "$f" | cut -d' ' -f1)
    entries=$(python3 -c "import sys,zipfile; print(len(zipfile.ZipFile(sys.argv[1]).namelist()))" "$f")
    echo "$f size=$size sha256=$sha entries=$entries"
done
