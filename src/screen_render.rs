fn svc_screen_render(
    game: &crate::game::Game<'_>,
    batch: &mut crate::hud::HudBatch,
    screen: &crate::mod_api::NativeScreenView<'_>,
) {
    crate::game::simple_voice_chat::on_screen_render(game, batch, screen);
}
