fn client_packet(
    game: &mut crate::game::Game<'_>,
    packet: &horizon_client::mod_api::ClientboundPlayPacket,
) {
    if let horizon_protocol::packets::play::Clientbound::CustomPayload(p) = packet {
        if p.channel.starts_with("voicechat:") {
            let host_hint = game.remote.as_ref().map(|r| r.session.address.as_str());
            crate::game::simple_voice_chat::on_net(&p.channel, &p.data, host_hint);
        }
    }
}
