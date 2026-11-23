fn svc_settings(mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id == "simple_voice_chat" {
        crate::game::simple_voice_chat::on_settings(buttons);
    }
}
