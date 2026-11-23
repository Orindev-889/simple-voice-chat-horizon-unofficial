fn svc_screen_drag(
    screen: &crate::mod_api::NativeScreenView<'_>,
    button: i32,
    delta: [f32; 2],
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::simple_voice_chat::on_screen_mouse_drag(screen, button, delta, modifiers, canceled);
}
