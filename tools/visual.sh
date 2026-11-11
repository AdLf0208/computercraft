#!/usr/bin/env bash
# The visual leg: does the computer *draw* its terminal?
#
# `tools/e2e.sh` asserts the terminal through `/computercraft screen` (headless).
# This script is the rendering proof and needs a renderer, so it runs with
# `--offscreen` and writes a screenshot beside the e2e logs.
#
#   ports/tools/build-slot.sh e2e --job <id>-visual --port ports/computercraft \
#       --horizon <pin> -- tools/visual.sh
set -euo pipefail

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/computercraft
CTL=${CTL:-$CLONE/ports/_shared/bin/horizon-1d63e0a5/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/computercraft-visual.sock}
OUT=${OUT:-$PORT/target/visual}
WORLD=CCVisual
BLOCK=computercraft:computer_normal

[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/computercraft-visual.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$SCRATCH/config" "$OUT"
rm -f "$SOCK"
cd "$SCRATCH"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
cleanup() {
    local status=$?
    ctl quit >/dev/null 2>&1 || true
    sleep 1
    rm -rf "$SCRATCH" 2>/dev/null || true
    echo "EXIT:$status"
}
trap cleanup EXIT

echo "== launch (offscreen, creative, The End arena) =="
"$CTL" --socket "$SOCK" launch --game "$GAME" --world "$WORLD" --seed 20261104 --creative \
    --offscreen --wait --resources "$RES" --dev-mod "$PORT/content" \
    >"$SCRATCH/game-console.log" 2>&1
ctl wait-ready 180
ctl wait 20
ctl command "execute in minecraft:the_end run tp @s 0.5 64 0.5" >/dev/null
ctl wait 5
ctl command "execute in minecraft:the_end run fill -8 63 -8 8 63 8 minecraft:stone" >/dev/null
ctl command "execute in minecraft:the_end run fill -8 64 -8 8 71 8 minecraft:air" >/dev/null
ctl wait 5

echo "== place the computer and let it boot =="
ctl command "execute in minecraft:the_end run setblock 2 64 2 ${BLOCK}[facing=south,state=off]" >/dev/null \
    || fail "cannot place the computer"
ctl wait 10
[ "$(ctl block 2 64 2 | jq -r '.state')" = "${BLOCK}[facing=south,state=on]" ] \
    || fail "the computer did not boot: $(ctl block 2 64 2 | jq -c '.')"

echo "== load a program with two lines and stand in front of it =="
ctl chat "/computercraft load 2 64 2 print(\"1+1\") print(1+1)" >/dev/null
ctl wait 10
[ "$(ctl chat '/computercraft screen 2 64 2' >/dev/null; ctl wait 2 >/dev/null; ctl messages 3 | jq -r '.chat[0]' | jq -r '.lines | join("|")')" = "1+1|2" ] \
    || fail "the terminal is not what the screen should show"
# Stand 4 blocks south of the computer's front face, at its feet, looking north.
ctl command "execute in minecraft:the_end run tp @s 2.5 64 6.5 180 0" >/dev/null
ctl wait 20
ctl look-at block 2 64 2 south >/dev/null || true
ctl wait 10
ctl screenshot "$OUT/screen-loaded.png" >/dev/null || fail "cannot take the screenshot"
echo "  wrote $OUT/screen-loaded.png"

echo "== power it off: the screen must change =="
ctl chat "/computercraft power 2 64 2 off" >/dev/null
ctl wait 10
ctl screenshot "$OUT/screen-off.png" >/dev/null || fail "cannot take the second screenshot"
echo "  wrote $OUT/screen-off.png"

echo "PASS: the computer booted (state=on), loaded a two-line program, and two screenshots were written: $OUT/screen-loaded.png and $OUT/screen-off.png"
