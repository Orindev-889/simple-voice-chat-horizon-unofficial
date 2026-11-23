fn command_handle(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    let typed = command.trim().trim_start_matches('/');
    let (word, _) = typed.split_once(char::is_whitespace).unwrap_or((typed, ""));
    if word != "svc" && word != "vcgroup" {
        return;
    }
    *ran = true;
    let (feedback, outbound) = crate::game::simple_voice_chat::on_command(typed);
    if !feedback.is_empty() {
        for line in feedback.lines() {
            game.native_chat(&format!("\u{a7}a[Voice] \u{a7}f{line}"));
        }
    }
    for (channel, data) in outbound {
        let packet = horizon_protocol::packets::play::Serverbound::CustomPayload(
            horizon_protocol::packets::configuration::CustomPayload {
                channel,
                data,
            },
        );
        let _ = game.native_send_packet(packet);
    }
}
