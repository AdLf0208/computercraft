// Game-side glue for CC: Tweaked slice 1: the computer's block entity, its
// terminal, the four `/computercraft` commands and the client-side screen.
//
// Injected into `crate::game` as `computercraft_state`; it needs the game, so
// it is not part of the standalone library (only `runtime.rs` is).
//
// The block entity keeps its program and its terminal lines in its saved
// compound (`native_block_entity_tick`, API 17), boots the program once when
// it is placed or told to run (`ComputerBlockEntity`'s server start upstream),
// and follows upstream's `facing` from the placer
// (`AbstractComputerBlock.getStateForPlacement`: the computer faces the player).

use horizon_client::containers::{BlockEntity, ModBlockEntity};
use horizon_client::text::Text;
use horizon_world::level::PlacementContext;
use horizon_world::nbt::{Compound, JavaString, Tag};
use horizon_world::registry::BlockStateId;

use crate::game::native_server::{NativeCommand, NativeServerPlayer};

use crate::game::computercraft::{self, TERM_LINES, TERM_WIDTH, Terminal, facing_for_placement};

/// The one block this slice registers.
pub const BLOCK: &str = "computercraft:computer_normal";
/// Its block entity's type id, which the client sees too.
pub const ENTITY: &str = "computercraft:computer";
/// The program a fresh computer runs: there is no ROM in this slice, so it
/// reports the version the ROM's `os.version()` answers and how to load one
/// (`data/computercraft/lua/bios.lua:38-40`).
const DEFAULT_PROGRAM: &str =
    "print(\"CraftOS 1.9\") print(\"no ROM: see\") print(\"/computercraft\")";
/// Site of the drawn glyphs as a fraction of a block: 6 px of advance at 1/96
/// puts 16 columns across the face, and 9 px of line height fits 10 rows.
const FONT_SCALE: f32 = 1.0 / 96.0;
const LINE_PIXELS: f32 = 9.0;

fn key(name: &str) -> JavaString {
    JavaString::from(name)
}

/// What a computer keeps in its saved compound.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Computer {
    program: String,
    buffer: Vec<String>,
    running: bool,
    powered: bool,
}

fn read(entity: &ModBlockEntity) -> Computer {
    let program = match entity.nbt.get(&key("Program")) {
        Some(Tag::String(text)) => text.to_string_lossy(),
        _ => DEFAULT_PROGRAM.to_owned(),
    };
    let buffer = match entity.nbt.get(&key("Buffer")) {
        Some(Tag::List(entries)) => entries
            .iter()
            .filter_map(|entry| match entry {
                Tag::String(text) => Some(text.to_string_lossy()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let flag = |name: &str, default: bool| match entity.nbt.get(&key(name)) {
        Some(Tag::Byte(value)) => *value != 0,
        _ => default,
    };
    Computer {
        program,
        buffer,
        running: flag("Running", false),
        powered: flag("Powered", true),
    }
}

fn write(state: &Computer, entity: &mut ModBlockEntity) {
    entity.nbt.insert(
        key("Program"),
        Tag::String(JavaString::from(state.program.as_str())),
    );
    entity.nbt.insert(
        key("Buffer"),
        Tag::List(
            state
                .buffer
                .iter()
                .map(|line| Tag::String(JavaString::from(line.as_str())))
                .collect(),
        ),
    );
    entity
        .nbt
        .insert(key("Running"), Tag::Byte(if state.running { 1 } else { 0 }));
    entity
        .nbt
        .insert(key("Powered"), Tag::Byte(if state.powered { 1 } else { 0 }));
}

/// The client reads the buffer from what the server sent (`update_tag`).
fn buffer_of(entity: &ModBlockEntity) -> Vec<String> {
    read(entity).buffer
}

/// `EntityBlock.newBlockEntity` (`native_block_entity`, API 15): a computer
/// block gets this port's block entity with the default program.
pub fn on_block_entity(block: &str, entity: &mut Option<(String, Compound)>) {
    if block != BLOCK || entity.is_some() {
        return;
    }
    let state = Computer {
        program: DEFAULT_PROGRAM.to_owned(),
        buffer: Vec::new(),
        running: false,
        powered: true,
    };
    let mut nbt = Compound::new();
    nbt.insert(
        key("Program"),
        Tag::String(JavaString::from(state.program.as_str())),
    );
    nbt.insert(key("Buffer"), Tag::List(Vec::new()));
    nbt.insert(key("Running"), Tag::Byte(0));
    nbt.insert(key("Powered"), Tag::Byte(1));
    *entity = Some((ENTITY.to_owned(), nbt));
}

/// `EntityBlock.getTicker` (`native_block_entity_ticks`, API 15): it ticks.
pub fn on_block_entity_ticks(block: &str, ticks: &mut bool) {
    if block == BLOCK {
        *ticks = true;
    }
}

/// One server tick (`native_block_entity_tick`, API 17): a powered computer
/// that has not run its program yet runs it now and keeps the terminal.
#[allow(private_interfaces)]
pub fn on_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut ModBlockEntity,
) {
    if entity.id != ENTITY {
        return;
    }
    let state = read(entity);
    if !state.powered || state.running {
        return;
    }
    let mut terminal = Terminal::new(TERM_WIDTH, TERM_LINES);
    if let Err(error) = computercraft::run(&state.program, &mut terminal) {
        terminal.write_line(&format!("error: {error}"));
    }
    let finished = Computer {
        buffer: terminal.lines().to_vec(),
        running: true,
        ..state
    };
    write(&finished, entity);
    let _ = game.native_set_block_property(pos, "state", "on");
    // `Level.sendBlockUpdated`: the client's copy of the block entity only gets
    // a new tag when its block changes, so the terminal is pushed explicitly.
    game.native_block_entity_sync(pos);
}

/// What clients get of the computer (`BlockEntity.getUpdateTag`, API 45): the
/// terminal, so the screen draws what the server ran.
pub fn on_block_entity_update_tag(entity: &ModBlockEntity, tag: &mut Option<Compound>) {
    if entity.id != ENTITY {
        return;
    }
    let state = read(entity);
    let mut sent = Compound::new();
    sent.insert(
        key("Buffer"),
        Tag::List(
            state
                .buffer
                .iter()
                .map(|line| Tag::String(JavaString::from(line.as_str())))
                .collect(),
        ),
    );
    sent.insert(key("Powered"), Tag::Byte(if state.powered { 1 } else { 0 }));
    *tag = Some(sent);
}

/// Where the screen is drawn on the block's `facing` face: the lower-left
/// corner of the face in the pose's coordinates, and how far to turn the pose
/// about y so the glyphs face outwards and read left to right.
fn screen_pose(facing: &str) -> ([f32; 3], f32) {
    match facing {
        "south" => ([0.0, 0.94, 1.008], 0.0),
        "east" => ([1.008, 0.94, 1.0], 90.0),
        "west" => ([-0.008, 0.94, 0.0], 270.0),
        _ => ([1.0, 0.94, -0.008], 180.0),
    }
}

/// The computer's screen (`BlockEntityRenderer.submit`, API 71): its terminal
/// lines drawn on the front face with the font atlas (`native_draw_text`).
#[allow(private_interfaces)]
pub fn on_block_entity_submit(
    game: &crate::game::Game<'_>,
    block: &str,
    state: BlockStateId,
    entity: &ModBlockEntity,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
) {
    if block != BLOCK || entity.id != ENTITY {
        return;
    }
    let lines = buffer_of(entity);
    if lines.is_empty() {
        return;
    }
    let facing = game
        .native_block_state_property(state, "facing")
        .unwrap_or("north");
    let (origin, degrees) = screen_pose(facing);
    draw.push();
    draw.translate(origin);
    draw.rotate_about([0.0, 1.0, 0.0], degrees, [0.0, 0.0, 0.0]);
    draw.scale([FONT_SCALE, -FONT_SCALE, FONT_SCALE]);
    for (index, line) in lines.iter().enumerate() {
        let at = [0.0, index as f32 * LINE_PIXELS];
        let _ = game.native_draw_text(draw, &Text::literal(line.clone()), at, 0x00_d8d8d8, true, 0);
    }
    draw.pop();
}

/// The placer's computer faces them, as `getHorizontalDirection().getOpposite()`
/// does upstream (`AbstractComputerBlock.getStateForPlacement`).
#[allow(private_interfaces)]
pub fn on_state_for_placement(
    level: &crate::game::GameLevel<'_>,
    state: BlockStateId,
    context: &PlacementContext,
    answer: &mut Option<Option<BlockStateId>>,
) {
    let Some(block) = level.native_state_block(state) else {
        return;
    };
    if block != BLOCK {
        return;
    }
    let facing = facing_for_placement(context.yaw);
    if let Some(placed) = level.native_state_with_property(state, "facing", facing) {
        *answer = Some(Some(placed));
    }
}

/// The four usages. `/computercraft load` takes the program as its last,
/// greedy argument, so it can hold spaces (a chat line cannot hold a newline:
/// separate statements with `;`).
pub fn on_register_commands(commands: &mut Vec<NativeCommand>) {
    commands.push(NativeCommand::new(
        "computercraft load <pos:block_pos> <program:text>",
    ));
    commands.push(NativeCommand::new("computercraft run <pos:block_pos>"));
    commands.push(NativeCommand::new("computercraft screen <pos:block_pos>"));
    commands.push(NativeCommand::new(
        "computercraft power <pos:block_pos> <switch:word>",
    ));
}

fn parse_pos(words: &[&str]) -> Option<[i32; 3]> {
    Some([
        words.first()?.parse().ok()?,
        words.get(1)?.parse().ok()?,
        words.get(2)?.parse().ok()?,
    ])
}

/// The computer at `pos`, its state and the block entity's cell.
#[allow(private_interfaces)]
fn computer_at(game: &crate::game::Game<'_>, pos: [i32; 3]) -> Option<Computer> {
    let key = (pos[0], pos[1], pos[2]);
    match game.world.block_entities.get(&key) {
        Some(BlockEntity::Mod(entity)) if entity.id == ENTITY => Some(read(entity)),
        _ => None,
    }
}

#[allow(private_interfaces)]
fn edit_computer(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    edit: impl FnOnce(&mut Computer),
) -> bool {
    let key = (pos[0], pos[1], pos[2]);
    let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get_mut(&key) else {
        return false;
    };
    if entity.id != ENTITY {
        return false;
    }
    let mut state = read(entity);
    edit(&mut state);
    write(&state, entity);
    true
}

fn screen_json(pos: [i32; 3], state: &Computer) -> String {
    serde_json::json!({
        "block": BLOCK,
        "pos": pos,
        "powered": state.powered,
        "running": state.running,
        "lines": state.buffer,
    })
    .to_string()
}

/// A player typed one of our lines: answer it, or refuse with the reason.
#[allow(private_interfaces)]
pub fn on_perform_command(
    game: &mut crate::game::Game<'_>,
    _player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    let words: Vec<&str> = command.split_whitespace().collect();
    let Some(rest) = words.get(2..) else {
        return;
    };
    let sub = words.get(1).copied().unwrap_or_default();
    let (pos_words, tail) = match sub {
        "load" => match rest.split_at_checked(3) {
            Some((pos, program)) => (pos, program),
            None => return,
        },
        "run" | "screen" => (rest, &[][..]),
        "power" => match rest.split_at_checked(3) {
            Some((pos, switch)) => (pos, switch),
            None => return,
        },
        _ => return,
    };
    let Some(pos) = parse_pos(pos_words) else {
        return;
    };
    *ran = true;
    let Some(state) = computer_at(game, pos) else {
        game.native_feedback(Text::literal(format!(
            "No computer at {},{},{}.",
            pos[0], pos[1], pos[2]
        )));
        return;
    };
    match sub {
        "screen" => {
            game.native_feedback(Text::literal(screen_json(pos, &state)));
        }
        "load" => {
            let program = tail.join(" ");
            if program.is_empty() {
                game.native_feedback(Text::literal(
                    "Usage: /computercraft load <x> <y> <z> <program>",
                ));
                return;
            }
            edit_computer(game, pos, |state| {
                state.program = program;
                state.buffer.clear();
                state.running = false;
            });
            game.native_block_entity_sync(pos);
            game.native_feedback(Text::literal("Program loaded; it runs on the next tick."));
        }
        "run" => {
            edit_computer(game, pos, |state| {
                state.buffer.clear();
                state.running = false;
            });
            game.native_block_entity_sync(pos);
            game.native_feedback(Text::literal("Running again."));
        }
        "power" => {
            let on = match tail {
                ["on"] => true,
                ["off"] => false,
                _ => {
                    game.native_feedback(Text::literal(
                        "Usage: /computercraft power <x> <y> <z> on|off",
                    ));
                    return;
                }
            };
            edit_computer(game, pos, |state| {
                state.powered = on;
                state.buffer.clear();
                state.running = false;
            });
            // Off while it boots: the tick that runs the program lights it.
            let _ = game.native_set_block_property(pos, "state", "off");
            game.native_block_entity_sync(pos);
            game.native_feedback(Text::literal(if on {
                "Computer powered on."
            } else {
                "Computer powered off."
            }));
        }
        _ => {}
    }
}
