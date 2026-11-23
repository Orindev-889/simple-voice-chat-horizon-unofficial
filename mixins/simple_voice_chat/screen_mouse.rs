fn svc_screen_mouse(
    screen: &crate::mod_api::NativeScreenView<'_>,
    button: i32,
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::simple_voice_chat::on_screen_mouse_click(screen, button, modifiers, canceled);
}
