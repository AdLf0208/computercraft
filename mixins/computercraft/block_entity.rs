// Runs at the head of `Game::native_block_entity` (API 15): a computer block
// gets its block entity, holding the program and the terminal.
fn observe_block_entity(block: &str, entity: &mut Option<(String, horizon_world::nbt::Compound)>) {
    crate::game::computercraft_state::on_block_entity(block, entity);
}
