// Runs at the head of the input key hook (listen). Modifier bits use the GLFW encoding.
fn input_key(game: &mut crate::game::Game<'_>, event: &winit::event::KeyEvent, modifiers: u32) {
    crate::game::simple_voice_chat::on_key(game, event, modifiers);
}
