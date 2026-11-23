fn svc_screen_typed(
    game: &crate::game::Game<'_>,
    screen: &crate::game::native_hooks::NativeScreenView<'_>,
    c: char,
    _modifiers: u32,
    canceled: &mut bool,
) -> std::ops::ControlFlow<()> {
    if !game.native_on("simple_voice_chat") || screen.id != "mod" {
        return std::ops::ControlFlow::Continue(());
    }
    crate::game::simple_voice_chat::on_screen_typed(screen, c, canceled);
    std::ops::ControlFlow::Continue(())
}
