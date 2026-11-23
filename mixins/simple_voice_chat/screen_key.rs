fn svc_screen_key(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    key: &str,
    repeat: bool,
    modifiers: u32,
    canceled: &mut bool,
) -> std::ops::ControlFlow<()> {
    if !game.native_on("simple_voice_chat") || screen.id != "mod" {
        return std::ops::ControlFlow::Continue(());
    }
    crate::game::simple_voice_chat::on_screen_key(screen, key, repeat, modifiers, canceled);
    std::ops::ControlFlow::Continue(())
}
