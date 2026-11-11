// Runs at the head of `Game::native_block_entity_submit` (API 71): the
// computer draws its terminal on its front face with the font atlas.
fn observe_block_entity_submit(
    game: &crate::game::Game<'_>,
    block: &str,
    state: horizon_world::registry::BlockStateId,
    entity: &horizon_client::containers::ModBlockEntity,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
) {
    crate::game::computercraft_state::on_block_entity_submit(game, block, state, entity, draw);
}
