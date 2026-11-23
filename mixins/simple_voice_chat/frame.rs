fn frame(game: &mut crate::game::Game<'_>, elapsed: std::time::Duration) {
    crate::game::simple_voice_chat::on_frame(game, elapsed);
    let mut retry = Vec::new();
    for (channel, data) in crate::game::simple_voice_chat::drain_outbound() {
        let packet = horizon_protocol::packets::play::Serverbound::CustomPayload(
            horizon_protocol::packets::configuration::CustomPayload {
                channel: channel.clone(),
                data: data.clone(),
            },
        );
        if game.native_send_packet(packet).is_err() {
            retry.push((channel, data));
        }
    }
    for (channel, data) in retry {
        crate::game::simple_voice_chat::queue_outbound(channel, data);
    }
}
