// Runs at the head of `GameLevel::native_state_for_placement` (API 76): a
// placed computer faces the player, as `getStateForPlacement` does upstream.
fn observe_state_for_placement(
    level: &crate::game::GameLevel<'_>,
    state: horizon_world::registry::BlockStateId,
    context: &horizon_world::level::PlacementContext,
    answer: &mut Option<Option<horizon_world::registry::BlockStateId>>,
) {
    crate::game::computercraft_state::on_state_for_placement(level, state, context, answer);
}
