fn svc_hud(
    game: &mut crate::game::Game<'_>,
    batch: &mut crate::hud::HudBatch,
    corners: &mut crate::extras_hud::Corners,
) {
    crate::game::simple_voice_chat::on_hud(game, batch, corners);
}
