// Game-side module for the Simple Voice Chat port.
// Thin shell over the svc_client bundle crate: protocol, session and audio live there; this
// module bridges Horizon's hooks to it and queues outbound packets for the net send path.

    use std::sync::Mutex;
    use std::time::Instant;
    #[allow(unused_imports)]
    use ::simple_voice_chat::svc_client;
    pub use svc_client::runtime::{state, Glue};
    static OUT: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());
    fn locked() -> std::sync::MutexGuard<'static, Glue> {
        state().lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn queue_outbound(channel: String, body: Vec<u8>) {
        OUT.lock().unwrap_or_else(|e| e.into_inner()).push((channel, body));
    }
    pub fn drain_outbound() -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut *OUT.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn world_open_ready() {
        let mut g = locked();
        g.world_open();
        let (channel, body) = Glue::handshake();
        queue_outbound(channel, body);
        let _ = g.start_audio();
    }
    pub fn world_closed() {
        locked().world_close();
    }
    pub fn world_lifecycle(
        _game: &mut crate::game::Game<'_>,
        phase: &crate::game::native_lifecycle::WorldPhase,
    ) {
        use crate::game::native_lifecycle::WorldPhase::{
            WorldCloseFailed, WorldCloseFinal, WorldOpenReady,
        };
        match phase {
            WorldOpenReady => world_open_ready(),
            WorldCloseFinal | WorldCloseFailed => world_closed(),
            _ => {}
        }
    }
    pub fn on_net(channel: &str, data: &[u8], hint: Option<&str>) {
        locked().on_net(channel, data, hint, Instant::now());
    }
    pub fn on_player_submit(
        _player: &crate::game::native_mob_draw::NativeMobRender<'_>,
        _who: &crate::game::native_mob_draw::NativePlayerRender<'_>,
    ) {}
    pub fn on_ptt(pressed: bool) {
        let mut g = locked();
        g.ptt_held = pressed;
    }
    pub use svc_client::runtime::PlayerVoiceStatus;
    pub fn player_voice_status_by_uuid_or_name(uuid: Option<u128>, name: &str) -> Option<PlayerVoiceStatus> {
        let g = locked();
        let connected = g.session.as_ref().map(|s| s.state() == svc_client::session::State::Connected).unwrap_or(false);
        if !connected {
            return None;
        }
        let now = Instant::now();
        let target_uuid = uuid.or_else(|| g.uuid_by_name.get(name).copied());
        if let Some(u) = target_uuid {
            Some(g.player_voice_status(u, now))
        } else {
            Some(PlayerVoiceStatus::Disconnected)
        }
    }
    pub fn on_mute_toggle() {
        let mut g = locked();
        g.cfg.muted = !g.cfg.muted;
        let muted = g.cfg.muted;
        if let Some(s) = &mut g.session {
            s.set_muted(muted);
        }
        Glue::save_config("config", &g.cfg);
    }
    pub fn on_key(_event: &winit::event::KeyEvent, _modifiers: u32) {}
    pub fn on_command(command: &str) -> (String, Vec<(String, Vec<u8>)>) {
        locked().on_command(command)
    }
    pub fn take_screen_open_requested() -> bool {
        let mut g = locked();
        let req = g.screen_open_requested;
        g.screen_open_requested = false;
        req
    }
    pub fn on_respawn() {
        let mut g = locked();
        g.session = None;
        g.last_handshake_attempt = None;
        g.reconnect_pending = true;
        g.handshake_attempts = 0;
        g.last_drop_reason = None;
    }
    pub fn on_frame(game: &mut crate::game::Game<'_>, _elapsed: std::time::Duration) {
        let is_open = game.native_mod_screen() == Some("simple_voice_chat");
        let (open_req, close_req) = {
            let mut g = locked();
            g.mod_screen_open = is_open;
            let o = g.screen_open_requested;
            let c = g.screen_close_requested;
            g.screen_open_requested = false;
            g.screen_close_requested = false;
            (o, c)
        };
        if open_req && !is_open {
            game.native_mod_screen_open("simple_voice_chat");
        } else if close_req && is_open {
            game.close_mod_screen();
        }
        let is_ingame = game.player.is_some() || game.remote.as_ref().map(|r| r.joined).unwrap_or(false);
        if is_ingame {
            let mut g = locked();
            let is_connected = g.session.as_ref().map(|s| s.state() == svc_client::session::State::Connected).unwrap_or(false);
            let is_connecting = g.session.as_ref().map(|s| s.state() == svc_client::session::State::Connecting).unwrap_or(false);
            let is_active = is_connected || is_connecting;
            let needs_handshake = (!is_active || g.reconnect_pending) && (
                g.last_handshake_attempt.is_none()
                || g.reconnect_pending
                || g.last_handshake_attempt.as_ref().map(|t| t.elapsed() >= std::time::Duration::from_secs(4)).unwrap_or(false)
            );
            if needs_handshake {
                g.last_handshake_attempt = Some(Instant::now());
                g.reconnect_pending = false;
                g.handshake_attempts = g.handshake_attempts.saturating_add(1);
                if !is_active {
                    g.session = None;
                }
                let _ = g.start_audio();
                queue_outbound(
                    "minecraft:register".to_string(),
                    b"voicechat:secret\0voicechat:request_secret\0voicechat:client_state\0voicechat:update_state\0voicechat:state\0voicechat:states\0voicechat:remove_state\0voicechat:create_group\0voicechat:set_group\0voicechat:leave_group\0voicechat:joined_group\0voicechat:add_group\0voicechat:remove_group".to_vec(),
                );
                let (channel, body) = Glue::handshake();
                queue_outbound(channel, body);
            }
        } else {
            let mut g = locked();
            if g.session.is_some() || g.server_addr.is_some() {
                g.world_close();
            }
        }
        if let Some(p) = &game.player {
            let mut g = locked();
            if let Some(s) = &mut g.session {
                s.set_listener_orientation([p.position.x, p.position.y, p.position.z], game.yaw);
            }
        }
        if let Some(r) = &game.remote {
            let positions = r.player_positions();
            if !positions.is_empty() {
                let mut g = locked();
                if let Some(s) = &mut g.session {
                    for (uuid, pos) in positions {
                        s.set_position(uuid, pos);
                    }
                }
            }
        }
        let _notes = {
            let mut g = locked();
            let _ = g.on_frame(Instant::now());
            for (ch, data) in g.pending_sends.drain(..) {
                queue_outbound(ch, data);
            }
            std::mem::take(&mut g.notes)
        };
    }
    fn in_rect(pos: [f32; 2], rect: [f32; 4]) -> bool {
        let [x, y, w, h] = rect;
        pos[0] >= x && pos[0] <= x + w && pos[1] >= y && pos[1] <= y + h
    }
    fn truncate_str(s: &str, max_len: usize) -> String {
        if s.chars().count() <= max_len {
            s.to_string()
        } else {
            let prefix: String = s.chars().take(max_len.saturating_sub(3)).collect();
            format!("{prefix}...")
        }
    }
    fn draw_btn(
        game: &crate::game::Game<'_>,
        batch: &mut crate::hud::HudBatch,
        rect: [f32; 4],
        text: &str,
        hovered: bool,
        active: bool,
        accent: Option<u32>,
    ) {
        let [x, y, w, h] = rect;
        game.native_fill(batch, [x, y, w, h], 0xFF000000);
        let bg = if let Some(c) = accent {
            c
        } else if active {
            0xFF2A5530
        } else if hovered {
            0xFF454550
        } else {
            0xFF2A2A30
        };
        game.native_fill(batch, [x + 1.0, y + 1.0, w - 2.0, h - 2.0], bg);
        let top_highlight = if hovered { 0xFF656575 } else { 0xFF454550 };
        game.native_fill(batch, [x + 1.0, y + 1.0, w - 2.0, 1.0], top_highlight);
        game.native_fill(batch, [x + 1.0, y + 1.0, 1.0, h - 2.0], top_highlight);
        let bottom_shadow = 0xFF141418;
        game.native_fill(batch, [x + 1.0, y + h - 2.0, w - 2.0, 1.0], bottom_shadow);
        game.native_fill(batch, [x + w - 2.0, y + 1.0, 1.0, h - 2.0], bottom_shadow);
        let text_w = game.native_text_width(text);
        let text_x = x + (w - text_w) / 2.0;
        let text_y = y + (h - 8.0) / 2.0;
        let text_color = if hovered { 0xFFFFA0 } else { 0xFFFFFF };
        game.native_text(batch, text, [text_x, text_y], text_color, true);
    }
    fn draw_text_input(
        game: &crate::game::Game<'_>,
        batch: &mut crate::hud::HudBatch,
        rect: [f32; 4],
        text: &str,
        placeholder: &str,
        focused: bool,
        is_password: bool,
    ) {
        let [x, y, w, h] = rect;
        game.native_fill(batch, [x, y, w, h], if focused { 0xFF5588AA } else { 0xFF000000 });
        game.native_fill(batch, [x + 1.0, y + 1.0, w - 2.0, h - 2.0], 0xFF141418);
        let display_text = if text.is_empty() {
            placeholder.to_string()
        } else if is_password {
            "*".repeat(text.chars().count())
        } else {
            text.to_string()
        };
        let color = if text.is_empty() { 0x666677 } else { 0xFFFFFF };
        let ty = y + (h - 8.0) / 2.0;
        game.native_text(batch, &display_text, [x + 6.0, ty], color, false);
        if focused {
            let cursor_x = x + 6.0 + if text.is_empty() { 0.0 } else { game.native_text_width(&display_text) };
            game.native_fill(batch, [cursor_x, ty, 1.0, 9.0], 0xFFFFA0);
        }
    }
    struct Layout {
        dialog: [f32; 4],
        tab_btn: [f32; 4],
        mic_prev: [f32; 4],
        mic_btn: [f32; 4],
        mic_next: [f32; 4],
        spk_prev: [f32; 4],
        spk_btn: [f32; 4],
        spk_next: [f32; 4],
        mode_btn: [f32; 4],
        mode_arg_btn: [f32; 4],
        bar_rect: [f32; 4],
        mic_minus: [f32; 4],
        mic_plus: [f32; 4],
        spk_minus: [f32; 4],
        spk_plus: [f32; 4],
        test_btn: [f32; 4],
        mute_btn: [f32; 4],
        hud_btn: [f32; 4],
        group_action_btn: [f32; 4],
        group_list_btns: [[f32; 4]; 4],
        done_btn: [f32; 4],
        volumes_btn: [f32; 4],
        create_name_box: [f32; 4],
        create_pw_box: [f32; 4],
        create_type_btn: [f32; 4],
        create_submit_btn: [f32; 4],
        create_cancel_btn: [f32; 4],
        join_pw_box: [f32; 4],
        join_pw_submit_btn: [f32; 4],
        join_pw_cancel_btn: [f32; 4],
        vol_search_box: [f32; 4],
        vol_row_minus: [[f32; 4]; 4],
        vol_row_plus: [[f32; 4]; 4],
        vol_row_mute: [[f32; 4]; 4],
        vol_prev_btn: [f32; 4],
        vol_next_btn: [f32; 4],
    }
    fn compute_layout(sw: f32, sh: f32) -> Layout {
        let dw = 420.0f32.min(sw - 16.0);
        let dh = 245.0f32.min(sh - 16.0);
        let dx = (sw - dw) / 2.0;
        let dy = (sh - dh) / 2.0;
        let hw = (dw - 28.0) / 2.0;
        let tw = (dw - 32.0) / 3.0;
        let btn_w = (dw - 50.0) / 2.0;
        let mut vol_row_minus = [[0.0; 4]; 4];
        let mut vol_row_plus = [[0.0; 4]; 4];
        let mut vol_row_mute = [[0.0; 4]; 4];
        for i in 0..4 {
            let ry = dy + 56.0 + i as f32 * 34.0;
            vol_row_minus[i] = [dx + dw - 150.0, ry, 20.0, 20.0];
            vol_row_plus[i] = [dx + dw - 80.0, ry, 20.0, 20.0];
            vol_row_mute[i] = [dx + dw - 56.0, ry, 44.0, 20.0];
        }
        Layout {
            dialog: [dx, dy, dw, dh],
            tab_btn: [dx + dw - 75.0, dy + 4.0, 65.0, 16.0],
            volumes_btn: [dx + 12.0, dy + 4.0, 70.0, 16.0],
            mic_prev: [dx + 12.0, dy + 38.0, 20.0, 20.0],
            mic_btn: [dx + 34.0, dy + 38.0, dw - 68.0, 20.0],
            mic_next: [dx + dw - 32.0, dy + 38.0, 20.0, 20.0],
            spk_prev: [dx + 12.0, dy + 72.0, 20.0, 20.0],
            spk_btn: [dx + 34.0, dy + 72.0, dw - 68.0, 20.0],
            spk_next: [dx + dw - 32.0, dy + 72.0, 20.0, 20.0],
            mode_btn: [dx + 12.0, dy + 97.0, hw, 20.0],
            mode_arg_btn: [dx + 16.0 + hw, dy + 97.0, hw, 20.0],
            bar_rect: [dx + 75.0, dy + 121.0, dw - 87.0, 10.0],
            mic_minus: [dx + 12.0, dy + 136.0, 20.0, 20.0],
            mic_plus: [dx + hw - 8.0, dy + 136.0, 20.0, 20.0],
            spk_minus: [dx + 16.0 + hw, dy + 136.0, 20.0, 20.0],
            spk_plus: [dx + dw - 32.0, dy + 136.0, 20.0, 20.0],
            test_btn: [dx + 12.0, dy + 160.0, tw, 20.0],
            mute_btn: [dx + 16.0 + tw, dy + 160.0, tw, 20.0],
            hud_btn: [dx + 20.0 + tw * 2.0, dy + 160.0, tw, 20.0],
            group_action_btn: [dx + (dw - 180.0) / 2.0, dy + 180.0, 180.0, 20.0],
            group_list_btns: [
                [dx + dw - 70.0, dy + 55.0, 58.0, 18.0],
                [dx + dw - 70.0, dy + 80.0, 58.0, 18.0],
                [dx + dw - 70.0, dy + 105.0, 58.0, 18.0],
                [dx + dw - 70.0, dy + 130.0, 58.0, 18.0],
            ],
            done_btn: [dx + (dw - 120.0) / 2.0, dy + 215.0, 120.0, 20.0],
            create_name_box: [dx + 20.0, dy + 50.0, dw - 40.0, 20.0],
            create_pw_box: [dx + 20.0, dy + 95.0, dw - 40.0, 20.0],
            create_type_btn: [dx + 20.0, dy + 138.0, dw - 40.0, 20.0],
            create_submit_btn: [dx + 20.0, dy + 200.0, btn_w, 20.0],
            create_cancel_btn: [dx + 30.0 + btn_w, dy + 200.0, btn_w, 20.0],
            join_pw_box: [dx + 20.0, dy + 75.0, dw - 40.0, 20.0],
            join_pw_submit_btn: [dx + 20.0, dy + 200.0, btn_w, 20.0],
            join_pw_cancel_btn: [dx + 30.0 + btn_w, dy + 200.0, btn_w, 20.0],
            vol_search_box: [dx + 20.0, dy + 28.0, dw - 40.0, 20.0],
            vol_row_minus,
            vol_row_plus,
            vol_row_mute,
            vol_prev_btn: [dx + 20.0, dy + 205.0, 65.0, 20.0],
            vol_next_btn: [dx + dw - 85.0, dy + 205.0, 65.0, 20.0],
        }
    }
    pub fn on_screen_render(
        game: &crate::game::Game<'_>,
        batch: &mut crate::hud::HudBatch,
        screen: &crate::mod_api::NativeScreenView<'_>,
    ) {
        if game.native_mod_screen() != Some("simple_voice_chat") {
            return;
        }
        let sw = screen.rect[2];
        let sh = screen.rect[3];
        let m = screen.mouse;
        let l = compute_layout(sw, sh);
        let [dx, dy, dw, dh] = l.dialog;
        game.native_fill(batch, [0.0, 0.0, sw, sh], 0x88000000);
        game.native_fill(batch, [dx - 2.0, dy - 2.0, dw + 4.0, dh + 4.0], 0xFF000000);
        game.native_fill(batch, [dx, dy, dw, dh], 0xF01E1E22);
        game.native_fill(batch, [dx, dy, dw, 24.0], 0xF016161A);

        let (screen_tab, current_group, current_members, available_groups, gui_feedback) = {
            let g = locked();
            let c_grp = g.group_name.clone();
            let mut members = Vec::new();
            if let Some(gid) = g.group_id {
                for st in g.player_states.values() {
                    if st.group == Some(gid) {
                        members.push(st.name.clone());
                    }
                }
            }
            members.sort();
            let mut grps: Vec<(u128, String, bool, usize)> = g.groups.values().map(|grp| {
                let cnt = g.player_states.values().filter(|s| s.group == Some(grp.id)).count();
                (grp.id, grp.name.clone(), grp.has_password, cnt)
            }).collect();
            grps.sort_by(|a, b| a.1.cmp(&b.1));
            (g.screen_tab, c_grp, members, grps, g.gui_feedback.clone())
        };

        if screen_tab == 1 {
            let title = "Voice Chat Groups";
            game.native_text(batch, title, [dx + 16.0, dy + 8.0], 0xFFFFFF, true);
            draw_btn(game, batch, l.tab_btn, "< Settings", in_rect(m, l.tab_btn), false, Some(0xFF334466));

            if let Some(ref grp_name) = current_group {
                game.native_fill(batch, [dx + 12.0, dy + 35.0, dw - 24.0, 24.0], 0x78204020);
                let cur_txt = format!("Current Group: {grp_name}");
                game.native_text(batch, &cur_txt, [dx + 20.0, dy + 42.0], 0x55FF55, true);

                let mem_title = format!("Members ({}):", current_members.len());
                game.native_text(batch, &mem_title, [dx + 16.0, dy + 70.0], 0xAAAAAA, false);
                let mem_list = if current_members.is_empty() {
                    "(only you)".to_string()
                } else {
                    current_members.join(", ")
                };
                let mem_display = truncate_str(&mem_list, 45);
                game.native_text(batch, &mem_display, [dx + 20.0, dy + 88.0], 0xFFFFFF, false);

                let hint = "Group members hear each other across all distances.";
                let hw = game.native_text_width(hint);
                game.native_text(batch, hint, [dx + (dw - hw) / 2.0, dy + 145.0], 0x88AAAA, false);

                draw_btn(game, batch, l.group_action_btn, "Leave Group", in_rect(m, l.group_action_btn), false, Some(0xFF662222));
            } else {
                game.native_text(batch, "Available Groups:", [dx + 16.0, dy + 36.0], 0xAAAAAA, false);
                if available_groups.is_empty() {
                    game.native_text(batch, "No active groups on server.", [dx + 20.0, dy + 65.0], 0x777788, false);
                    game.native_text(batch, "Create one below or run /vcgroup create <name>", [dx + 20.0, dy + 85.0], 0x888899, false);
                } else {
                    for (i, (_gid, gname, has_pw, count)) in available_groups.iter().take(4).enumerate() {
                        let gy = dy + 55.0 + (i as f32) * 25.0;
                        let pw_mark = if *has_pw { " [PW]" } else { "" };
                        let label = format!("{gname}{pw_mark} ({count} players)");
                        game.native_text(batch, &label, [dx + 20.0, gy + 4.0], 0xFFFFFF, false);
                        let btn = l.group_list_btns[i];
                        draw_btn(game, batch, btn, "Join", in_rect(m, btn), false, Some(0xFF225533));
                    }
                }
                draw_btn(game, batch, l.group_action_btn, "+ Create Group", in_rect(m, l.group_action_btn), false, Some(0xFF225533));
            }

            if let Some(ref fb) = gui_feedback {
                let fw = game.native_text_width(fb);
                game.native_text(batch, fb, [dx + (dw - fw) / 2.0, dy + 198.0], 0xFFFFAA, false);
            }

            draw_btn(game, batch, l.done_btn, "[ Done ]", in_rect(m, l.done_btn), false, None);
            return;
        }

        if screen_tab == 2 {
            let title = "Create Voice Chat Group";
            game.native_text(batch, title, [dx + 16.0, dy + 8.0], 0xFFFFFF, true);
            draw_btn(game, batch, l.tab_btn, "< Back", in_rect(m, l.tab_btn), false, Some(0xFF334466));

            let (name, pw, kind, focus) = {
                let g = locked();
                (g.create_group_name.clone(), g.create_group_password.clone(), g.create_group_type, g.create_group_focus)
            };

            game.native_text(batch, "Group Name (required):", [dx + 20.0, dy + 36.0], 0xAAAAAA, false);
            draw_text_input(game, batch, l.create_name_box, &name, "Enter group name...", focus == 0, false);

            game.native_text(batch, "Password (optional):", [dx + 20.0, dy + 80.0], 0xAAAAAA, false);
            draw_text_input(game, batch, l.create_pw_box, &pw, "Leave empty for public group", focus == 1, true);

            game.native_text(batch, "Group Type:", [dx + 20.0, dy + 124.0], 0xAAAAAA, false);
            let type_str = match kind {
                svc_client::protocol::GroupKind::Normal => "Type: Normal (Default)",
                svc_client::protocol::GroupKind::Open => "Type: Open (Proximity active)",
                svc_client::protocol::GroupKind::Isolated => "Type: Isolated (No proximity)",
            };
            draw_btn(game, batch, l.create_type_btn, type_str, in_rect(m, l.create_type_btn), false, Some(0xFF2E3B4E));

            let desc = match kind {
                svc_client::protocol::GroupKind::Normal => "Hear group anywhere + nearby players. Outsiders cannot hear group.",
                svc_client::protocol::GroupKind::Open => "Hear group anywhere. Proximity chat active both ways.",
                svc_client::protocol::GroupKind::Isolated => "Only hear group. Proximity chat completely disabled.",
            };
            game.native_text(batch, desc, [dx + 20.0, dy + 165.0], 0x88AAAA, false);

            if let Some(ref fb) = gui_feedback {
                let fw = game.native_text_width(fb);
                game.native_text(batch, fb, [dx + (dw - fw) / 2.0, dy + 180.0], 0xFFFFAA, false);
            }

            let can_create = !name.trim().is_empty();
            let submit_bg = if can_create { Some(0xFF225533) } else { Some(0xFF333338) };
            draw_btn(game, batch, l.create_submit_btn, "Create Group", in_rect(m, l.create_submit_btn), false, submit_bg);
            draw_btn(game, batch, l.create_cancel_btn, "Cancel", in_rect(m, l.create_cancel_btn), false, Some(0xFF442222));
            return;
        }

        if screen_tab == 3 {
            let title = "Enter Group Password";
            game.native_text(batch, title, [dx + 16.0, dy + 8.0], 0xFFFFFF, true);
            draw_btn(game, batch, l.tab_btn, "< Back", in_rect(m, l.tab_btn), false, Some(0xFF334466));

            let (target_name, pw_input, err_msg) = {
                let g = locked();
                let t_name = g.join_password_target.as_ref().map(|(_, n)| n.clone()).unwrap_or_else(|| "Group".into());
                (t_name, g.join_password_input.clone(), g.join_password_error.clone())
            };

            let prompt = format!("Enter password to join '{target_name}':");
            game.native_text(batch, &prompt, [dx + 20.0, dy + 52.0], 0xDDDDDD, false);
            draw_text_input(game, batch, l.join_pw_box, &pw_input, "Enter password...", true, true);

            if let Some(ref err) = err_msg {
                game.native_text(batch, err, [dx + 20.0, dy + 105.0], 0xFF5555, true);
            } else {
                game.native_text(batch, "Press Enter or click Join", [dx + 20.0, dy + 105.0], 0x888899, false);
            }

            draw_btn(game, batch, l.join_pw_submit_btn, "Join Group", in_rect(m, l.join_pw_submit_btn), false, Some(0xFF225533));
            draw_btn(game, batch, l.join_pw_cancel_btn, "Cancel", in_rect(m, l.join_pw_cancel_btn), false, Some(0xFF442222));
            return;
        }

        if screen_tab == 4 {
            let title = "Adjust Player Volumes";
            game.native_text(batch, title, [dx + 16.0, dy + 8.0], 0xFFFFFF, true);
            draw_btn(game, batch, l.tab_btn, "< Back", in_rect(m, l.tab_btn), false, Some(0xFF334466));

            let (search, page, all_players) = {
                let g = locked();
                let search = g.volume_search.clone();
                let page = g.volume_page;
                let local = g.local_player;
                let mut players: Vec<(u128, String, bool)> = g.player_states.values()
                    .filter(|p| Some(p.uuid) != local)
                    .map(|p| {
                        let is_speaking = g.last_voice_packet.get(&p.uuid)
                            .map(|t| t.elapsed() < std::time::Duration::from_millis(500))
                            .unwrap_or(false);
                        (p.uuid, p.name.clone(), is_speaking)
                    })
                    .collect();
                if !search.trim().is_empty() {
                    let s_low = search.trim().to_lowercase();
                    players.retain(|(_, name, _)| name.to_lowercase().contains(&s_low));
                }
                players.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
                (search, page, players)
            };

            // Search box
            draw_text_input(game, batch, l.vol_search_box, &search, "Search player by name...", true, false);

            let total_players = all_players.len();
            let page_size = 4;
            let total_pages = if total_players == 0 { 1 } else { (total_players + page_size - 1) / page_size };
            let current_page = page.min(total_pages.saturating_sub(1));

            let page_slice: Vec<(u128, String, bool)> = all_players.into_iter()
                .skip(current_page * page_size)
                .take(page_size)
                .collect();

            if page_slice.is_empty() {
                let msg = if search.is_empty() { "No other players online" } else { "No players match search" };
                let mw = game.native_text_width(msg);
                game.native_text(batch, msg, [dx + (dw - mw) / 2.0, dy + 110.0], 0x888899, false);
            } else {
                for (i, (uuid, name, is_speaking)) in page_slice.into_iter().enumerate() {
                    let row_y = dy + 56.0 + i as f32 * 34.0;
                    game.native_fill(batch, [dx + 16.0, row_y - 2.0, dw - 32.0, 24.0], 0x44222228);

                    let dot_color = if is_speaking { 0x44FF44 } else { 0x666677 };
                    game.native_fill(batch, [dx + 22.0, row_y + 4.0, 6.0, 12.0], dot_color);

                    let name_color = if is_speaking { 0xAAFFAA } else { 0xEEEEEE };
                    game.native_text(batch, &name, [dx + 34.0, row_y + 4.0], name_color, false);

                    let vol = locked().get_player_vol(uuid);
                    let is_muted = vol <= 0.001;

                    draw_btn(game, batch, l.vol_row_minus[i], "-", in_rect(m, l.vol_row_minus[i]), false, None);

                    let vol_txt = if is_muted {
                        "MUTED".to_string()
                    } else {
                        format!("{:.0}%", vol * 100.0)
                    };
                    let vtw = game.native_text_width(&vol_txt);
                    let vol_color = if is_muted { 0xFF5555 } else if vol > 1.0 { 0xFFAA44 } else { 0xFFFFFF };
                    let lbl_cx = dx + dw - 115.0;
                    game.native_text(batch, &vol_txt, [lbl_cx - vtw / 2.0, row_y + 5.0], vol_color, false);

                    draw_btn(game, batch, l.vol_row_plus[i], "+", in_rect(m, l.vol_row_plus[i]), false, None);

                    let (mute_txt, mute_bg) = if is_muted {
                        ("Unmute", Some(0xFF225533))
                    } else {
                        ("Mute", Some(0xFF442222))
                    };
                    draw_btn(game, batch, l.vol_row_mute[i], mute_txt, in_rect(m, l.vol_row_mute[i]), false, mute_bg);
                }
            }

            if current_page > 0 {
                draw_btn(game, batch, l.vol_prev_btn, "< Prev", in_rect(m, l.vol_prev_btn), false, Some(0xFF334466));
            }
            let page_txt = format!("Page {} / {} ({} players)", current_page + 1, total_pages, total_players);
            let ptw = game.native_text_width(&page_txt);
            game.native_text(batch, &page_txt, [dx + (dw - ptw) / 2.0, dy + 206.0], 0xAAAAAA, false);
            if current_page + 1 < total_pages {
                draw_btn(game, batch, l.vol_next_btn, "Next >", in_rect(m, l.vol_next_btn), false, Some(0xFF334466));
            }

            return;
        }

        let title = "Simple Voice Chat Settings";
        let tw = game.native_text_width(title);
        game.native_text(batch, title, [dx + (dw - tw) / 2.0, dy + 8.0], 0xFFFFFF, true);
        draw_btn(game, batch, l.volumes_btn, "Volumes", in_rect(m, l.volumes_btn), false, Some(0xFF334466));
        draw_btn(game, batch, l.tab_btn, "Groups >", in_rect(m, l.tab_btn), false, Some(0xFF334466));
        let (
            mic_name,
            spk_name,
            mode,
            ptt_key,
            threshold,
            mic_vol,
            spk_vol,
            test_mode,
            muted,
            show_hud,
            last_peak,
            is_connected,
            is_connecting,
            server_addr,
            drop_reason,
            attempts,
            has_secret,
        ) = {
            let mut g = locked();
            if g.cached_input_devices.is_empty() {
                g.refresh_devices();
            }
            let conn = g
                .session
                .as_ref()
                .map(|s| s.state() == svc_client::session::State::Connected)
                .unwrap_or(false);
            let connecting = g
                .session
                .as_ref()
                .map(|s| s.state() == svc_client::session::State::Connecting)
                .unwrap_or(false);
            (
                g.active_input_device(),
                g.active_output_device(),
                g.cfg.mode,
                game.native_mod_key("simple_voice_chat", "simple_voice_chat.ptt", "key.keyboard.v"),
                g.cfg.threshold,
                g.cfg.volume,
                g.cfg.speaker_volume,
                g.test_mode,
                g.cfg.muted,
                g.cfg.show_hud,
                g.last_peak,
                conn,
                connecting,
                g.server_addr,
                g.last_drop_reason,
                g.handshake_attempts,
                g.session.is_some(),
            )
        };
        game.native_text(batch, "Microphone:", [dx + 12.0, dy + 27.0], 0xAAAAAA, false);
        draw_btn(game, batch, l.mic_prev, "<", in_rect(m, l.mic_prev), false, None);
        let mic_display = format!("Mic: {}", truncate_str(& mic_name, 32));
        draw_btn(
            game,
            batch,
            l.mic_btn,
            &mic_display,
            in_rect(m, l.mic_btn),
            false,
            None,
        );
        draw_btn(game, batch, l.mic_next, ">", in_rect(m, l.mic_next), false, None);
        game.native_text(
            batch,
            "Speaker / Output:",
            [dx + 12.0, dy + 61.0],
            0xAAAAAA,
            false,
        );
        draw_btn(game, batch, l.spk_prev, "<", in_rect(m, l.spk_prev), false, None);
        let spk_display = format!("Spk: {}", truncate_str(& spk_name, 32));
        draw_btn(
            game,
            batch,
            l.spk_btn,
            &spk_display,
            in_rect(m, l.spk_btn),
            false,
            None,
        );
        draw_btn(game, batch, l.spk_next, ">", in_rect(m, l.spk_next), false, None);
        let mode_txt = match mode {
            svc_client::runtime::ActivationMode::Ptt => "Mode: Push-To-Talk",
            svc_client::runtime::ActivationMode::Voice => "Mode: Voice Activation",
        };
        draw_btn(game, batch, l.mode_btn, mode_txt, in_rect(m, l.mode_btn), false, None);
        let mode_arg_txt = match mode {
            svc_client::runtime::ActivationMode::Ptt => {
                let key = ptt_key.strip_prefix("key.keyboard.").or_else(|| ptt_key.strip_prefix("Key")).unwrap_or(&ptt_key).to_uppercase();
                format!("PTT Key: [{key}]")
            }
            svc_client::runtime::ActivationMode::Voice => {
                let pct = (threshold as f32 / 327.67).clamp(0.0, 100.0);
                format!("Threshold: ~{pct:.0}%")
            }
        };
        draw_btn(
            game,
            batch,
            l.mode_arg_btn,
            &mode_arg_txt,
            in_rect(m, l.mode_arg_btn),
            false,
            None,
        );
        game.native_text(batch, "Mic Level:", [dx + 12.0, dy + 122.0], 0xAAAAAA, false);
        let [bx, by, bw, bh] = l.bar_rect;
        game.native_fill(batch, [bx - 1.0, by - 1.0, bw + 2.0, bh + 2.0], 0xFF000000);
        game.native_fill(batch, [bx, by, bw, bh], 0xFF141418);
        let level_ratio = (last_peak as f32 / 32767.0).clamp(0.0, 1.0);
        let fill_w = bw * level_ratio;
        if fill_w > 0.0 {
            let fill_color = if level_ratio > 0.85 {
                0xFFE74C3C
            } else if level_ratio > 0.6 {
                0xFFF1C40F
            } else {
                0xFF2ECC71
            };
            game.native_fill(batch, [bx, by, fill_w, bh], fill_color);
        }
        let thresh_ratio = (threshold as f32 / 32767.0).clamp(0.0, 1.0);
        let thresh_x = (bx + bw * thresh_ratio).clamp(bx, bx + bw - 2.0);
        game.native_fill(batch, [thresh_x - 1.0, by - 2.0, 3.0, bh + 4.0], 0xFFFF7700);
        let hw = (dw - 28.0) / 2.0;
        draw_btn(game, batch, l.mic_minus, "-", in_rect(m, l.mic_minus), false, None);
        let mic_vol_txt = format!("Mic: {:.0}%", mic_vol * 100.0);
        game.native_fill(batch, [dx + 34.0, dy + 136.0, hw - 44.0, 20.0], 0xFF202026);
        let mvw = game.native_text_width(&mic_vol_txt);
        game.native_text(
            batch,
            &mic_vol_txt,
            [dx + 34.0 + (hw - 44.0 - mvw) / 2.0, dy + 142.0],
            0xFFFFFF,
            true,
        );
        draw_btn(game, batch, l.mic_plus, "+", in_rect(m, l.mic_plus), false, None);
        draw_btn(game, batch, l.spk_minus, "-", in_rect(m, l.spk_minus), false, None);
        let spk_vol_txt = format!("Voice: {:.0}%", spk_vol * 100.0);
        game.native_fill(
            batch,
            [dx + 38.0 + hw, dy + 136.0, hw - 44.0, 20.0],
            0xFF202026,
        );
        let svw = game.native_text_width(&spk_vol_txt);
        game.native_text(
            batch,
            &spk_vol_txt,
            [dx + 38.0 + hw + (hw - 44.0 - svw) / 2.0, dy + 142.0],
            0xFFFFFF,
            true,
        );
        draw_btn(game, batch, l.spk_plus, "+", in_rect(m, l.spk_plus), false, None);
        let test_accent = if test_mode { Some(0xFF2A6030) } else { None };
        let test_txt = if test_mode { "Test: [ACTIVE]" } else { "Test: OFF" };
        draw_btn(
            game,
            batch,
            l.test_btn,
            test_txt,
            in_rect(m, l.test_btn),
            test_mode,
            test_accent,
        );
        let mute_accent = if muted { Some(0xFF702828) } else { None };
        let mute_txt = if muted { "Mute: [ON]" } else { "Mute: OFF" };
        draw_btn(
            game,
            batch,
            l.mute_btn,
            mute_txt,
            in_rect(m, l.mute_btn),
            muted,
            mute_accent,
        );
        let hud_txt = if show_hud { "HUD: [ON]" } else { "HUD: OFF" };
        draw_btn(game, batch, l.hud_btn, hud_txt, in_rect(m, l.hud_btn), show_hud, None);
        let status_txt = if is_connected {
            "Server: Connected".to_string()
        } else if is_connecting {
            "Server: Connecting...".to_string()
        } else if !has_secret && attempts > 0 {
            "Server: Connecting...".to_string()
        } else if let Some(reason) = drop_reason {
            format!("Server: Disconnected ({reason})")
        } else {
            "Server: Disconnected (Local microphone test ready)".to_string()
        };
        let stw = game.native_text_width(&status_txt);
        game.native_text(
            batch,
            &status_txt,
            [dx + (dw - stw) / 2.0, dy + 188.0],
            0x8899AA,
            false,
        );
        draw_btn(
            game,
            batch,
            l.done_btn,
            "[ Done ]",
            in_rect(m, l.done_btn),
            false,
            None,
        );
    }
    pub fn on_screen_mouse_click(
        screen: &crate::mod_api::NativeScreenView<'_>,
        button: i32,
        _modifiers: u32,
        canceled: &mut bool,
    ) {
        if !locked().mod_screen_open || button != 0 {
            return;
        }
        let sw = screen.rect[2];
        let sh = screen.rect[3];
        let m = screen.mouse;
        let l = compute_layout(sw, sh);
        let cur_tab = locked().screen_tab;
        if !in_rect(m, l.dialog) {
            let mut g = locked();
            if g.screen_tab >= 2 {
                g.screen_tab = if g.screen_tab == 4 { 0 } else { 1 };
            } else {
                g.screen_close_requested = true;
            }
            *canceled = true;
            return;
        }
        if in_rect(m, l.done_btn) && cur_tab <= 1 {
            locked().screen_close_requested = true;
            *canceled = true;
            return;
        }
        if in_rect(m, l.tab_btn) {
            let mut g = locked();
            g.screen_tab = match g.screen_tab {
                0 => 1,
                1 => 0,
                4 => 0,
                _ => 1,
            };
            *canceled = true;
            return;
        }
        if cur_tab == 1 {
            let in_group = locked().group_name.is_some();
            if in_rect(m, l.group_action_btn) {
                let mut g = locked();
                if in_group {
                    g.group_id = None;
                    g.group_name = None;
                    queue_outbound(svc_client::protocol::LEAVE_GROUP.to_string(), svc_client::protocol::leave_group());
                    g.gui_feedback = Some("Left group".into());
                } else {
                    g.screen_tab = 2;
                    g.create_group_name.clear();
                    g.create_group_password.clear();
                    g.create_group_type = svc_client::protocol::GroupKind::Normal;
                    g.create_group_focus = 0;
                }
                *canceled = true;
                return;
            }
            let avail: Vec<(u128, String, bool)> = {
                let g = locked();
                let mut grps: Vec<(u128, String, bool)> = g.groups.values().map(|gr| (gr.id, gr.name.clone(), gr.has_password)).collect();
                grps.sort_by(|a, b| a.1.cmp(&b.1));
                grps.into_iter().take(4).collect()
            };
            for (i, (gid, gname, has_pw)) in avail.into_iter().enumerate() {
                if in_rect(m, l.group_list_btns[i]) {
                    let mut g = locked();
                    if has_pw {
                        g.screen_tab = 3;
                        g.join_password_target = Some((gid, gname));
                        g.join_password_input.clear();
                        g.join_password_error = None;
                    } else {
                        queue_outbound(
                            svc_client::protocol::SET_GROUP.to_string(),
                            svc_client::protocol::join_group(gid, None),
                        );
                        g.gui_feedback = Some(format!("Joining {gname}..."));
                    }
                    *canceled = true;
                    return;
                }
            }
            *canceled = true;
            return;
        }
        if cur_tab == 2 {
            // Create Group tab
            if in_rect(m, l.create_name_box) {
                locked().create_group_focus = 0;
                *canceled = true;
                return;
            }
            if in_rect(m, l.create_pw_box) {
                locked().create_group_focus = 1;
                *canceled = true;
                return;
            }
            if in_rect(m, l.create_type_btn) {
                locked().cycle_create_group_type();
                *canceled = true;
                return;
            }
            if in_rect(m, l.create_submit_btn) {
                let mut g = locked();
                if let Some((ch, body)) = g.submit_create_group() {
                    queue_outbound(ch, body);
                }
                *canceled = true;
                return;
            }
            if in_rect(m, l.create_cancel_btn) {
                locked().screen_tab = 1;
                *canceled = true;
                return;
            }
            *canceled = true;
            return;
        }
        if cur_tab == 3 {
            // Enter Password tab
            if in_rect(m, l.join_pw_box) {
                *canceled = true;
                return;
            }
            if in_rect(m, l.join_pw_submit_btn) {
                let mut g = locked();
                if let Some((ch, body)) = g.submit_join_password() {
                    queue_outbound(ch, body);
                }
                *canceled = true;
                return;
            }
            if in_rect(m, l.join_pw_cancel_btn) {
                let mut g = locked();
                g.screen_tab = 1;
                g.join_password_target = None;
                g.join_password_input.clear();
                g.join_password_error = None;
                *canceled = true;
                return;
            }
            *canceled = true;
            return;
        }
        if cur_tab == 4 {
            // Player Volumes tab
            if in_rect(m, l.vol_prev_btn) {
                let mut g = locked();
                if g.volume_page > 0 {
                    g.volume_page -= 1;
                }
                *canceled = true;
                return;
            }
            if in_rect(m, l.vol_next_btn) {
                let mut g = locked();
                g.volume_page += 1;
                *canceled = true;
                return;
            }

            let (page, players) = {
                let g = locked();
                let page = g.volume_page;
                let search = g.volume_search.trim().to_lowercase();
                let local = g.local_player;
                let mut pvec: Vec<(String, u128)> = g.player_states.values()
                    .filter(|p| Some(p.uuid) != local)
                    .filter(|p| search.is_empty() || p.name.to_lowercase().contains(&search))
                    .map(|p| (p.name.clone(), p.uuid))
                    .collect();
                pvec.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                (page, pvec)
            };
            let page_slice: Vec<(String, u128)> = players.into_iter().skip(page * 4).take(4).collect();
            for (i, (_name, uuid)) in page_slice.into_iter().enumerate() {
                if in_rect(m, l.vol_row_minus[i]) {
                    locked().adjust_player_vol(uuid, -0.1);
                    *canceled = true;
                    return;
                }
                if in_rect(m, l.vol_row_plus[i]) {
                    locked().adjust_player_vol(uuid, 0.1);
                    *canceled = true;
                    return;
                }
                if in_rect(m, l.vol_row_mute[i]) {
                    locked().toggle_player_mute(uuid);
                    *canceled = true;
                    return;
                }
            }
            *canceled = true;
            return;
        }
        if in_rect(m, l.volumes_btn) {
            let mut g = locked();
            g.screen_tab = 4;
            g.volume_page = 0;
            *canceled = true;
            return;
        }
        if in_rect(m, l.mic_prev) {
            let _ = locked().cycle_input_device(false);
            *canceled = true;
            return;
        }
        if in_rect(m, l.mic_btn) || in_rect(m, l.mic_next) {
            let _ = locked().cycle_input_device(true);
            *canceled = true;
            return;
        }
        if in_rect(m, l.spk_prev) {
            let _ = locked().cycle_output_device(false);
            *canceled = true;
            return;
        }
        if in_rect(m, l.spk_btn) || in_rect(m, l.spk_next) {
            let _ = locked().cycle_output_device(true);
            *canceled = true;
            return;
        }
        if in_rect(m, l.mode_btn) {
            let mut g = locked();
            g.cfg.mode = match g.cfg.mode {
                svc_client::runtime::ActivationMode::Ptt => {
                    svc_client::runtime::ActivationMode::Voice
                }
                svc_client::runtime::ActivationMode::Voice => {
                    svc_client::runtime::ActivationMode::Ptt
                }
            };
            Glue::save_config("config", &g.cfg);
            *canceled = true;
            return;
        }
        if in_rect(m, l.mode_arg_btn) {
            let mut g = locked();
            match g.cfg.mode {
                svc_client::runtime::ActivationMode::Ptt => {
                    let curr = g.cfg.ptt_key.as_str();
                    g.cfg.ptt_key = match curr {
                        "key.keyboard.v" | "KeyV" => "key.keyboard.b",
                        "key.keyboard.b" | "KeyB" => "key.keyboard.c",
                        "key.keyboard.c" | "KeyC" => "key.keyboard.g",
                        "key.keyboard.g" | "KeyG" => "key.keyboard.left.alt",
                        "key.keyboard.left.alt" => "key.mouse.button.4",
                        _ => "key.keyboard.v",
                    }.into();
                    Glue::save_config("config", &g.cfg);
                }
                svc_client::runtime::ActivationMode::Voice => {
                    let curr_pct = g.cfg.threshold as f32 / 327.67;
                    let next_pct = if curr_pct < 4.0 {
                        5.0
                    } else if curr_pct < 7.0 {
                        8.0
                    } else if curr_pct < 11.0 {
                        12.0
                    } else if curr_pct < 18.0 {
                        20.0
                    } else {
                        3.0
                    };
                    g.cfg.threshold = (next_pct * 327.67) as i16;
                    Glue::save_config("config", &g.cfg);
                }
            }
            *canceled = true;
            return;
        }
        if in_rect(m, l.bar_rect) {
            let [bx, _, bw, _] = l.bar_rect;
            let ratio = ((m[0] - bx) / bw).clamp(0.01, 1.0);
            let mut g = locked();
            g.cfg.threshold = (ratio * 32767.0) as i16;
            Glue::save_config("config", &g.cfg);
            *canceled = true;
            return;
        }
        if in_rect(m, l.mic_minus) {
            let mut g = locked();
            g.cfg.volume = (g.cfg.volume - 0.1).clamp(0.0, 2.0);
            Glue::save_config("config", &g.cfg);
            *canceled = true;
            return;
        }
        if in_rect(m, l.mic_plus) {
            let mut g = locked();
            g.cfg.volume = (g.cfg.volume + 0.1).clamp(0.0, 2.0);
            Glue::save_config("config", &g.cfg);
            *canceled = true;
            return;
        }
        if in_rect(m, l.spk_minus) {
            let mut g = locked();
            g.cfg.speaker_volume = (g.cfg.speaker_volume - 0.1).clamp(0.0, 2.0);
            Glue::save_config("config", &g.cfg);
            *canceled = true;
            return;
        }
        if in_rect(m, l.spk_plus) {
            let mut g = locked();
            g.cfg.speaker_volume = (g.cfg.speaker_volume + 0.1).clamp(0.0, 2.0);
            Glue::save_config("config", &g.cfg);
            *canceled = true;
            return;
        }
        if in_rect(m, l.test_btn) {
            let mut g = locked();
            g.test_mode = !g.test_mode;
            if g.test_mode {
                let _ = g.start_audio();
            }
            *canceled = true;
            return;
        }
        if in_rect(m, l.mute_btn) {
            let mut g = locked();
            g.cfg.muted = !g.cfg.muted;
            Glue::save_config("config", &g.cfg);
            *canceled = true;
            return;
        }
        if in_rect(m, l.hud_btn) {
            let mut g = locked();
            g.cfg.show_hud = !g.cfg.show_hud;
            Glue::save_config("config", &g.cfg);
            *canceled = true;
            return;
        }
        *canceled = true;
    }
    pub fn on_screen_mouse_drag(
        screen: &crate::mod_api::NativeScreenView<'_>,
        button: i32,
        _delta: [f32; 2],
        _modifiers: u32,
        canceled: &mut bool,
    ) {
        if !locked().mod_screen_open || button != 0 {
            return;
        }
        let sw = screen.rect[2];
        let sh = screen.rect[3];
        let m = screen.mouse;
        let l = compute_layout(sw, sh);
        let [bx, by, bw, bh] = l.bar_rect;
        if m[1] >= by - 6.0 && m[1] <= by + bh + 6.0 && m[0] >= bx - 10.0
            && m[0] <= bx + bw + 10.0
        {
            let ratio = ((m[0] - bx) / bw).clamp(0.01, 1.0);
            let mut g = locked();
            g.cfg.threshold = (ratio * 32767.0) as i16;
            Glue::save_config("config", &g.cfg);
            *canceled = true;
        }
    }
    pub fn on_screen_key(
        screen: &crate::mod_api::NativeScreenView<'_>,
        key: &str,
        _repeat: bool,
        _modifiers: u32,
        canceled: &mut bool,
    ) {
        if screen.id != "mod" {
            return;
        }
        let mut g = locked();
        if g.mod_screen_open {
            if g.key_pressed(key, false) {
                *canceled = true;
            }
        }
    }
    pub fn on_screen_typed(
        screen: &crate::mod_api::NativeScreenView<'_>,
        c: char,
        canceled: &mut bool,
    ) {
        if screen.id != "mod" {
            return;
        }
        let mut g = locked();
        if g.mod_screen_open {
            g.char_typed(c);
            *canceled = true;
        }
    }
    pub fn on_hud(
        game: &mut crate::game::Game<'_>,
        batch: &mut crate::hud::HudBatch,
        corners: &mut crate::extras_hud::Corners,
    ) {
        let ptt_key = game.native_mod_key("simple_voice_chat", "simple_voice_chat.ptt", "key.keyboard.v");
        let (show, connected, connecting, muted, is_talking, mode, test_mode, has_secret, attempts, drop_reason) = {
            let g = locked();
            let state = g.session.as_ref().map(|s| s.state());
            let connected = state == Some(svc_client::session::State::Connected);
            let connecting = state == Some(svc_client::session::State::Connecting);
            (
                g.cfg.show_hud,
                connected,
                connecting,
                g.cfg.muted,
                g.is_talking,
                g.cfg.mode,
                g.test_mode,
                g.session.is_some(),
                g.handshake_attempts,
                g.last_drop_reason,
            )
        };
        if !show {
            return;
        }
        if !game.native_corner_shown(crate::extras::Corner::BottomLeft) {
            return;
        }
        let (label, text_color, icon_color) = if test_mode {
            if is_talking {
                ("TEST: TALKING".to_string(), 0x55FF55, 0xFF55FF55)
            } else {
                ("TEST: SPEAK NOW".to_string(), 0xFFFF55, 0xFFFFFF55)
            }
        } else if connecting {
            ("Voice: Connecting...".to_string(), 0xAAAA55, 0xFFAAAA55)
        } else if !connected {
            if !has_secret && attempts > 0 {
                (format!("Voice: Awaiting secret (#{attempts})..."), 0x88AAAA, 0xFF7799AA)
            } else if let Some(reason) = drop_reason {
                (format!("Voice: Disconnected ({reason})"), 0xAA6666, 0xFFAA5555)
            } else {
                ("Voice: Disconnected".to_string(), 0x888888, 0xFF777777)
            }
        } else if muted {
            ("MUTED".to_string(), 0xFF5555, 0xFFFF4444)
        } else if is_talking {
            match mode {
                svc_client::runtime::ActivationMode::Voice => {
                    ("VOICE: TALKING".to_string(), 0x55FF55, 0xFF55FF55)
                }
                svc_client::runtime::ActivationMode::Ptt => {
                    ("PTT: TALKING".to_string(), 0x55FF55, 0xFF55FF55)
                }
            }
        } else {
            match mode {
                svc_client::runtime::ActivationMode::Voice => {
                    ("VOICE".to_string(), 0xAAAAAA, 0xFFAAAAAA)
                }
                svc_client::runtime::ActivationMode::Ptt => {
                    let key_name = ptt_key.strip_prefix("key.keyboard.").or_else(|| ptt_key.strip_prefix("Key")).unwrap_or(&ptt_key).to_uppercase();
                    (format!("PTT [{key_name}]"), 0xAAAAAA, 0xFFAAAAAA)
                }
            }
        };
        let text_w = game.native_text_width(&label);
        let w = 16.0 + text_w + 4.0;
        let h = 14.0;
        let (x, y) = corners.place(crate::extras::Corner::BottomLeft, w, h);
        game.native_fill(batch, [x, y, w, h], 0x78000000);
        game.native_fill(batch, [x + 5.0, y + 2.0, 4.0, 5.0], icon_color);
        game.native_fill(batch, [x + 3.0, y + 4.0, 1.0, 4.0], icon_color);
        game.native_fill(batch, [x + 3.0, y + 7.0, 8.0, 1.0], icon_color);
        game.native_fill(batch, [x + 10.0, y + 4.0, 1.0, 4.0], icon_color);
        game.native_fill(batch, [x + 6.0, y + 8.0, 2.0, 2.0], icon_color);
        game.native_fill(batch, [x + 4.0, y + 10.0, 6.0, 1.0], icon_color);
        if muted {
            let slash_color = 0xFFFF2222;
            game.native_fill(batch, [x + 3.0, y + 2.0, 2.0, 2.0], slash_color);
            game.native_fill(batch, [x + 5.0, y + 4.0, 2.0, 2.0], slash_color);
            game.native_fill(batch, [x + 7.0, y + 6.0, 2.0, 2.0], slash_color);
            game.native_fill(batch, [x + 9.0, y + 8.0, 2.0, 2.0], slash_color);
            game.native_fill(batch, [x + 11.0, y + 10.0, 2.0, 2.0], slash_color);
        }
        game.native_text(batch, &label, [x + 15.0, y + 3.0], text_color, true);

        let group_hud_info = {
            let g = locked();
            if let Some(ref gname) = g.group_name {
                let gid = g.group_id;
                let now = Instant::now();
                let mut talking_members = Vec::new();
                let mut total_members = 0;
                for st in g.player_states.values() {
                    if st.group == gid {
                        total_members += 1;
                        if g.is_player_talking(st.uuid, now) {
                            talking_members.push(st.name.clone());
                        }
                    }
                }
                Some((gname.clone(), total_members, talking_members))
            } else {
                None
            }
        };

        if let Some((gname, total, talking)) = group_hud_info {
            let grp_label = if talking.is_empty() {
                format!("Group: {gname} ({total})")
            } else {
                format!("Group: {gname} • {} talking", talking.join(", "))
            };
            let text_color = if talking.is_empty() { 0xAAAAFF } else { 0x55FF55 };
            let text_w = game.native_text_width(&grp_label);
            let gw = text_w + 10.0;
            let gh = 14.0;
            let (gx, gy) = corners.place(crate::extras::Corner::BottomLeft, gw, gh);
            game.native_fill(batch, [gx, gy, gw, gh], 0x78000000);
            game.native_fill(batch, [gx + 2.0, gy + 2.0, 3.0, gh - 4.0], text_color);
            game.native_text(batch, &grp_label, [gx + 7.0, gy + 3.0], text_color, true);
        }
    }
