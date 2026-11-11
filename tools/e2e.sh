#!/usr/bin/env bash
# In-game check for CC: Tweaked slice 1.
#
# The claim: a placed `computercraft:computer_normal` runs a program and its
# terminal is observable. The readout is the port's own command,
# `/computercraft screen <x> <y> <z>`, which answers one line of JSON with the
# terminal's lines (the block also *draws* them on its front face, but a
# headless run has no renderer: `tools/visual.sh` is the screenshot leg).
#
# Legs
#   default (EMBEDDED=0)  launch with `--dev-mod content/`
#   EMBEDDED=1            launch with **no** `--dev-mod`: the block and the data
#                         come from the content embedded in the `.hnmod`
#                         (`mixins/computercraft/data/computercraft-content.hmod`,
#                         the AE2 pattern); CONTROL=1 (default there) first
#                         launches the same binary with `.horizon-cache` unusable,
#                         where the block id must be unknown
#   SELFTEST=1            the same setup, with the `print(1+1)` answer asserted
#                         as "3": the run must fail, proving the check can
#
# The scenario, in The End on a stone floor (an empty level: no ambient items):
#   1. the block places with the state the setblock asked for, and the item exists
#   2. its default program boots and the terminal shows the two boot lines
#   3. `/computercraft load … print(1+1)` then `screen` gives "2"
#   4. a loop and a concatenation give "item 1|item 2|item 3"
#   5. the block's `state` reads `on` while it runs
#   6. CONTROL: `/computercraft power … off` empties the terminal and the same
#      program on the powered-off computer prints nothing; powering it back on
#      runs it again
#   7. CONTROL: `/computercraft screen` on a stone block says there is no computer
set -euo pipefail

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/computercraft
# The control binary is shared at the campaign pin; the game is this port's build.
CTL=${CTL:-$CLONE/ports/_shared/bin/horizon-1d63e0a5/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/computercraft-e2e.sock}
EMBEDDED=${EMBEDDED:-0}
CONTROL=${CONTROL:-$EMBEDDED}
SELFTEST=${SELFTEST:-0}
WORLD=${WORLD:-CCE2E}
CONTROL_WORLD=${CONTROL_WORLD:-${WORLD}Control}
BLOCK=computercraft:computer_normal
X=2; Y=64; Z=2        # the computer
CX=5; CZ=2            # a stone block, the "no computer here" control

[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }
[ -f "$PORT/content/horizon-mod.toml" ] || { echo "missing content mod: $PORT/content" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/computercraft-home.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$SCRATCH/config"
rm -f "$SOCK"
# The game folder is the working directory: keep saves/options out of the port.
cd "$SCRATCH"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

cleanup() {
    local status=$?
    ctl quit >/dev/null 2>&1 || true
    sleep 1
    rm -rf "$SCRATCH" 2>/dev/null || true
    # Last line of every run: `check-e2e-logs.py` reads it as the verdict.
    echo "EXIT:$status"
}
trap cleanup EXIT

launch() {
    local world=${1:-$WORLD}
    rm -f "$SOCK"
    local args=(--game "$GAME" --world "$world" --seed 20261104 --creative --headless --wait
                --resources "$RES")
    if [ "$EMBEDDED" = 1 ]; then
        echo "  launch: no --dev-mod; the bundle carries its own content" >&2
    else
        args+=(--dev-mod "$PORT/content")
    fi
    "$CTL" --socket "$SOCK" launch "${args[@]}" >"$SCRATCH/game-console.log" 2>&1
    ctl wait-ready 120
    ctl wait 10
    ctl command "execute in minecraft:the_end run tp @s 0.5 64 0.5" >/dev/null
    ctl wait 5
    ctl command "execute in minecraft:the_end run fill -8 63 -8 8 63 8 minecraft:stone" >/dev/null
    ctl command "execute in minecraft:the_end run tp @s 0.5 64 0.5" >/dev/null
    ctl wait 10
    ctl command "kill @e[type=minecraft:enderman]" >/dev/null 2>&1 || true
    ctl wait 3
}

# The terminal the server keeps, as the port's command reports it.
screen() {
    ctl chat "/computercraft screen $1 $2 $3" >/dev/null
    ctl wait 2 >/dev/null
    ctl messages 4 | jq -r '.chat[0]'
}
screen_lines() { screen "$@" | jq -r '.lines | join("|")'; }
screen_field() { local field=$1; shift; screen "$@" | jq -r ".$field"; }
# `ctl block` answers `block` (the name) and `state` (name plus properties).
block_state() { ctl block "$1" "$2" "$3" | jq -r '.state // "unknown"'; }
# A command's own answer (the reply the mod gives through `native_feedback`).
reply() {
    ctl chat "$1" >/dev/null
    ctl wait 2 >/dev/null
    ctl messages 4 | jq -r '.chat[0]'
}
load_program() {
    ctl chat "/computercraft load $X $Y $Z $1" >/dev/null
    ctl wait 4 >/dev/null
}
expect_lines() {
    local expected=$1 what=$2 got
    got=$(screen_lines "$X" "$Y" "$Z")
    [ "$got" = "$expected" ] || fail "$what: expected [$expected], got [$got] ($(screen "$X" "$Y" "$Z"))"
    echo "  $what: $got"
}

# The control for EMBEDDED=1: the same binary (every hook is in it) with the
# embedded content made unavailable — `.horizon-cache` is a plain file, so the
# helper can neither compare nor write its content and falls back to
# `Continue`. The block id must then be unknown.
control_leg() {
    echo "== control: the same game with no embedded content file =="
    rm -rf "$SCRATCH/.horizon-cache"
    : > "$SCRATCH/.horizon-cache"
    launch "$CONTROL_WORLD"
    ctl command "execute in minecraft:the_end run setblock $X $Y $Z $BLOCK" >/dev/null 2>&1 || true
    ctl wait 4 >/dev/null
    local placed
    placed=$(block_state "$X" "$Y" "$Z")
    [ "$placed" = "minecraft:air" ] \
        || fail "the control game placed a computer ($placed), so the content did not come from the embedded file"
    ctl command "execute in minecraft:the_end run setblock $X $Y $Z minecraft:stone" >/dev/null 2>&1 || true
    ctl command "execute in minecraft:the_end run setblock $CX $Y $CZ $BLOCK" >/dev/null 2>&1 || true
    ctl wait 2 >/dev/null
    [ "$(block_state "$CX" "$Y" "$CZ")" = "minecraft:air" ] \
        || fail "the control game knows the computer block id"
    [ -f "$SCRATCH/.horizon-cache" ] || fail "the control's .horizon-cache was replaced: the helper wrote its content"
    echo "  without the embedded file: no computer block, .horizon-cache stayed a file"
    ctl quit >/dev/null 2>&1 || true
    rm -f "$SCRATCH/.horizon-cache"
}

# EMBEDDED=1: the bundle must have written its content out at startup, byte for
# byte the delivered content mod.
embedded_content_check() {
    local cache=$SCRATCH/.horizon-cache/computercraft-embedded-content.hmod
    [ -f "$cache" ] || fail "the bundle did not write its embedded content to $cache"
    cmp -s "$cache" "$PORT/content/computercraft.hmod" \
        || fail "the embedded content differs from content/computercraft.hmod"
    echo "  embedded content: $(wc -c < "$cache" | tr -d ' ') B at $cache, identical to content/computercraft.hmod"
}

echo "== launch (creative, The End arena, $([ "$EMBEDDED" = 1 ] && echo 'native bundle only (embedded content)' || echo 'content/dev-mod')) =="
if [ "$CONTROL" = 1 ]; then control_leg; fi
launch
if [ "$EMBEDDED" = 1 ]; then embedded_content_check; fi

echo "== the item exists and the block places with the state asked for =="
ctl give "$BLOCK" 1 >/dev/null || fail "the content mod did not provide $BLOCK"
ctl wait 4 >/dev/null
ctl chat "/clear @s" >/dev/null
ctl command "execute in minecraft:the_end run setblock $X $Y $Z ${BLOCK}[facing=south,state=off]" >/dev/null \
    || fail "cannot place the computer"
ctl wait 4 >/dev/null
placed=$(block_state "$X" "$Y" "$Z")
case "$placed" in
    "$BLOCK[facing=south,state=on]"|"$BLOCK[facing=south,state=off]") echo "  placed: $placed" ;;
    *) fail "the computer did not place (got $placed)" ;;
esac
[ "$(block_state "$CX" "$Y" "$CZ")" = "minecraft:air" ] || fail "the control cell is not air"

echo "== its default program boots and the terminal shows it =="
expect_lines "CraftOS 1.9|no ROM: see|/computercraft" "boot terminal"
[ "$(screen_field running "$X" "$Y" "$Z")" = "true" ] || fail "the computer did not report that it ran"
[ "$(screen_field block "$X" "$Y" "$Z")" = "$BLOCK" ] || fail "the screen reply names another block"
if [ "$SELFTEST" = 1 ]; then
    echo "  selftest: asserting the boot terminal is something else (must FAIL)"
    [ "$(screen_lines "$X" "$Y" "$Z")" = "selftest" ] || fail "selftest: the boot terminal is not 'selftest'"
fi

echo "== print(1+1) shows 2 =="
load_program 'print(1+1)'
if [ "$SELFTEST" = 1 ]; then
    echo "  selftest: asserting print(1+1) answers 3 (must FAIL)"
    expect_lines "3" "selftest arithmetic"
else
    expect_lines "2" "print(1+1)"
fi

echo "== a loop and a concatenation =="
load_program 'local n=0 while n<3 do n=n+1 print("item " .. n) end'
expect_lines "item 1|item 2|item 3" "loop and concatenation"

echo "== the block is lit while it has run =="
lit=$(block_state "$X" "$Y" "$Z")
[ "$lit" = "$BLOCK[facing=south,state=on]" ] || fail "the running computer's state is $lit"
echo "  block state: $lit"

echo "== control: a powered-off computer runs nothing =="
ctl chat "/computercraft power $X $Y $Z off" >/dev/null
ctl wait 4 >/dev/null
[ "$(screen_field powered "$X" "$Y" "$Z")" = "false" ] || fail "the computer is not powered off"
[ "$(screen_lines "$X" "$Y" "$Z")" = "" ] || fail "powering off left a terminal: $(screen_lines "$X" "$Y" "$Z")"
before=$(block_state "$X" "$Y" "$Z")
[ "$before" = "$BLOCK[facing=south,state=off]" ] || fail "the powered-off computer's state is $before"
load_program 'print("broken")'
expect_lines "" "the powered-off computer printed nothing"
ctl wait 10 >/dev/null
expect_lines "" "still nothing ten ticks later"
echo "  powered off: $(screen "$X" "$Y" "$Z")"

echo "== control: a stone block is not a computer =="
ctl command "execute in minecraft:the_end run setblock $CX $Y $CZ minecraft:stone" >/dev/null
ctl wait 2 >/dev/null
answer=$(reply "/computercraft screen $CX $Y $CZ")
case "$answer" in
    "No computer at"*) echo "  $answer" ;;
    *) fail "a stone block answered the screen command: $answer" ;;
esac

echo "== powering it back on boots the loaded program =="
ctl chat "/computercraft power $X $Y $Z on" >/dev/null
ctl wait 6 >/dev/null
expect_lines "broken" "the loaded program ran after power-on"

echo "PASS: the computer placed, its default program booted to a readable terminal, print(1+1) answered 2, a loop and a concatenation answered item 1|item 2|item 3, the block read state=on while it ran, a powered-off computer ran nothing (its terminal stayed empty through a load and ten ticks), a stone block answered that there is no computer, and powering it back on ran the loaded program"
