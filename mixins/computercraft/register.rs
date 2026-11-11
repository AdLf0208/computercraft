// Declares the `/computercraft` usages (every player).
fn register_commands(commands: &mut Vec<NativeCommand>) {
    crate::game::computercraft_state::on_register_commands(commands);
}
