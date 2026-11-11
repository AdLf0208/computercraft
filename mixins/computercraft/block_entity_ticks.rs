// Runs at the head of `Game::native_block_entity_ticks` (API 15): the
// computer's block entity ticks.
fn observe_block_entity_ticks(block: &str, ticks: &mut bool) {
    crate::game::computercraft_state::on_block_entity_ticks(block, ticks);
}
