// Runs at the head of `Game::native_block_entity_tick` (API 17): a powered
// computer runs its program once and keeps the terminal.
fn observe_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut horizon_client::containers::ModBlockEntity,
) {
    crate::game::computercraft_state::on_block_entity_tick(game, pos, entity);
}
