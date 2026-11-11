# computercraft (Horizon port)

A Horizon port of [computercraft](cc-tweaked/CC-Tweaked) — CCPL-1.0.

**Status: first slices.** What is below is what the port actually does today.

## Implemented so far

- One block, one program, one observable:
- `computercraft:computer_normal` — `facing` (4) × `state` (`off`/`on`/`blinking`)
- = 12 states on the `minecraft:stone` template, upstream's model shape
- (`minecraft:block/orientable` with `computer_normal_front`/`_side`/`_top`;
- the running state uses upstream's lit `computer_on` face), self-drop loot,
- the upstream crafting shape rewritten to 26.3 ingredients, one creative tab.
- A block entity (`computercraft:computer`) keeping `Program`, `Buffer`,
- `Running`, `Powered` in its saved compound; it ticks, boots its program once,

## Not yet ported

- The ROM**: 105 Lua files (`bios.lua`, 20 `rom/apis`, 23 `rom/modules`, 60
- `rom/programs`) and the shell. This needs a real Lua 5.2 VM; rung A is
- blocked by the pin's lock (record above), so the engine is the unblocker.
- Language features**: tables, functions and closures, `break`, `repeat`,
- `in`/generic `for`, multiple returns and multiple assignment, metamethods,
- `pcall`/`error`, string/table/math/os libraries, `require`/`dofile`,
- coroutines (the basis of every CC API), varargs, `#` on tables, integer
- division `//`, `goto`, long strings `[[…]]`, hex literals, `\ddd` escapes.

## Install

Build the bundle with a native-mod capable launcher, or add the file from the
registry once it is listed. Pinned to Horizon `1d63e0a` and horizon-mixins
`5a64af93` for game 26.3.

## Build from source

```sh
cargo test
python3 tools/horizon.py check --horizon <horizon checkout>
python3 tools/horizon.py build --horizon <horizon checkout>
python3 tools/package.py
```

## Credits and licence

Original mod: cc-tweaked/CC-Tweaked. Port: AdLf0208. Licence: CCPL-1.0 — see [LICENSE](LICENSE);
the original notices are preserved.
