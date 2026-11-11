// Runs at the head of `Game::native_block_entity_update_tag` (API 45): clients
// get the terminal lines, so the block's screen draws what the server ran.
fn observe_block_entity_update_tag(
    entity: &horizon_client::containers::ModBlockEntity,
    tag: &mut Option<horizon_world::nbt::Compound>,
) {
    crate::game::computercraft_state::on_block_entity_update_tag(entity, tag);
}
