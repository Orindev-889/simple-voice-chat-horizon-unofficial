fn svc_settings_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    if mod_id == "simple_voice_chat" {
        crate::game::simple_voice_chat::on_settings_click(game, button);
    }
}
