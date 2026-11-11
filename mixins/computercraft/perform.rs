// Runs our `/computercraft` lines when a player types one.
fn perform_command(
    game: &mut crate::game::Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    crate::game::computercraft_state::on_perform_command(game, player, command, ran);
}
