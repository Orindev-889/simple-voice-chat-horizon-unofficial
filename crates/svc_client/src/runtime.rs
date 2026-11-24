//! Simple Voice Chat client for Horizon: the glue between Horizon's native
//! hooks and the `svc_client_*` crates (clean-room, original work, MIT).
//!
//! Horizon's thin helpers call into the shared [`Glue`] below; everything
//! testable is here and unit-tested. Not a copy of Matdoesteching's Simple
//! Voice Chat: it implements the same documented wire protocol.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::protocol as proto;
use crate::session::{Event, Session};

pub const PORT: &str = "simple-voice-chat";

/// Voice status for nametag icons and HUD display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerVoiceStatus {
    /// Actively speaking right now (audio packets received recently).
    Speaking,
    /// Microphone is muted.
    Muted,
    /// Player disabled voice chat in settings.
    Disabled,
    /// Player is not connected to voice server or has no mod installed.
    Disconnected,
    /// Connected to voice server and listening, but silent.
    Connected,
}

/// Information about a voice chat group advertised by the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupInfo {
    pub id: u128,
    pub name: String,
    pub has_password: bool,
    pub persistent: bool,
    pub hidden: bool,
    pub kind: proto::GroupKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ActivationMode {
    Ptt,
    Voice,
}

impl Default for ActivationMode {
    fn default() -> Self {
        ActivationMode::Ptt
    }
}

/// Client settings, persisted at `<config>/simple_voice_chat/config.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// The key that opens the mic while held (`KeyV`).
    pub ptt_key: String,
    /// Key that toggles microphone mute (`KeyM`).
    pub mute_key: String,
    pub muted: bool,
    /// Mic capture volume, 0..=2.0.
    pub volume: f32,
    /// Incoming voice playback volume, 0..=2.0.
    pub speaker_volume: f32,
    /// Activation mode: PTT or Voice activation
    pub mode: ActivationMode,
    /// Voice activation threshold (0..=32767).
    pub threshold: i16,
    /// Whether to display microphone status on HUD.
    pub show_hud: bool,
    /// Selected input device (microphone), or None for system default.
    pub input_device: Option<String>,
    /// Selected output device (speaker/headphones), or None for system default.
    pub output_device: Option<String>,
    /// Key that opens settings GUI (`KeyU`).
    pub gui_key: String,
    /// Per-player volume multipliers (0.0..=2.0). 1.0 is default, 0.0 is muted.
    #[serde(default)]
    pub player_volumes: HashMap<u128, f32>,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            ptt_key: "KeyV".into(),
            mute_key: "KeyM".into(),
            muted: false,
            volume: 1.0,
            speaker_volume: 1.0,
            mode: ActivationMode::Ptt,
            threshold: 1600,
            show_hud: true,
            input_device: None,
            output_device: None,
            gui_key: "KeyU".into(),
            player_volumes: HashMap::new(),
        }
    }
}

/// The mod's whole state, shared by the helpers behind a mutex.
pub struct Glue {
    pub cfg: VoiceConfig,
    pub session: Option<Session>,
    pub ptt_held: bool,
    pub is_talking: bool,
    pub test_mode: bool,
    pub voice_hold_frames: usize,
    pub last_peak: i16,
    pub screen_open_requested: bool,
    pub screen_close_requested: bool,
    pub mod_screen_open: bool,
    pub screen_tab: u8,
    pub create_group_name: String,
    pub create_group_password: String,
    pub create_group_type: proto::GroupKind,
    pub create_group_focus: u8,
    pub join_password_target: Option<(u128, String)>,
    pub join_password_input: String,
    pub join_password_error: Option<String>,
    pub volume_search: String,
    pub volume_page: usize,
    pub cached_input_devices: Vec<String>,
    pub cached_output_devices: Vec<String>,
    pub gui_feedback: Option<String>,
    /// uuid by player name, from the server's state packets.
    pub uuid_by_name: HashMap<String, u128>,
    pub name_by_uuid: HashMap<u128, String>,
    pub groups: HashMap<u128, GroupInfo>,
    pub player_states: HashMap<u128, proto::PlayerState>,
    pub last_voice_packet: HashMap<u128, Instant>,
    pub group_id: Option<u128>,
    pub group_name: Option<String>,
    pub server_addr: Option<SocketAddr>,
    pub local_player: Option<u128>,
    /// Lines for the chat box: group joins, players entering or leaving voice.
    pub notes: Vec<String>,
    /// Set when the UDP connection dropped; the frame helper re-handshakes.
    pub reconnect_pending: bool,
    /// Mod-channel packets waiting for the frame helper to send.
    pub pending_sends: Vec<(String, Vec<u8>)>,
    /// When the last handshake attempt was sent.
    pub last_handshake_attempt: Option<Instant>,
    pub last_drop_reason: Option<&'static str>,
    pub handshake_attempts: u32,
    /// Live microphone capture; None until start_audio succeeds.
    pub mic: Option<crate::audio_io::MicStream>,
    /// Live speaker output; None until start_audio succeeds.
    pub speaker: Option<crate::audio_io::Speaker>,
    /// The Opus codec, created with the audio devices.
    pub codec: Option<crate::audio::VoiceCodec>,
    /// Per-speaker Opus decoders, mapped by sender UUID.
    pub decoders: HashMap<u128, (crate::audio::VoiceDecoder, Instant)>,
}

impl Default for Glue {
    fn default() -> Self {
        Self {
            cfg: VoiceConfig::default(),
            session: None,
            ptt_held: false,
            is_talking: false,
            test_mode: false,
            voice_hold_frames: 0,
            last_peak: 0,
            screen_open_requested: false,
            screen_close_requested: false,
            mod_screen_open: false,
            screen_tab: 0,
            create_group_name: String::new(),
            create_group_password: String::new(),
            create_group_type: proto::GroupKind::Normal,
            create_group_focus: 0,
            join_password_target: None,
            join_password_input: String::new(),
            join_password_error: None,
            volume_search: String::new(),
            volume_page: 0,
            cached_input_devices: Vec::new(),
            cached_output_devices: Vec::new(),
            gui_feedback: None,
            uuid_by_name: HashMap::new(),
            name_by_uuid: HashMap::new(),
            groups: HashMap::new(),
            player_states: HashMap::new(),
            last_voice_packet: HashMap::new(),
            group_id: None,
            group_name: None,
            server_addr: None,
            local_player: None,
            notes: Vec::new(),
            reconnect_pending: false,
            pending_sends: Vec::new(),
            last_handshake_attempt: None,
            last_drop_reason: None,
            handshake_attempts: 0,
            mic: None,
            speaker: None,
            codec: None,
            decoders: HashMap::new(),
        }
    }
}

impl Glue {
    pub fn config_path(base: &str) -> String {
        format!("{}/simple_voice_chat/config.json", base)
    }

    pub fn load_config(base: &str) -> VoiceConfig {
        std::fs::read_to_string(Self::config_path(base))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save_config(base: &str, cfg: &VoiceConfig) {
        let _ = std::fs::create_dir_all(format!("{}/simple_voice_chat", base));
        if let Ok(t) = serde_json::to_string_pretty(cfg) {
            let _ = std::fs::write(Self::config_path(base), t);
        }
    }

    /// A world opened: drop any old session, the handshake goes out next.
    pub fn world_open(&mut self) {
        self.session = None;
        self.server_addr = None;
        self.group_id = None;
        self.group_name = None;
        self.groups.clear();
        self.player_states.clear();
        self.last_voice_packet.clear();
        self.ptt_held = false;
        self.is_talking = false;
        self.voice_hold_frames = 0;
        self.reconnect_pending = false;
        self.last_handshake_attempt = None;
        self.notes.clear();
        self.pending_sends.clear();
    }

    /// The world closed: the socket dies with it.
    pub fn world_close(&mut self) {
        if let Some(mut s) = self.session.take() {
            s.close();
        }
        self.server_addr = None;
        self.group_id = None;
        self.group_name = None;
        self.groups.clear();
        self.player_states.clear();
        self.last_voice_packet.clear();
        self.reconnect_pending = false;
        self.last_handshake_attempt = None;
    }

    /// The handshake request: the channel and body for `native_net_send_to_server`.
    pub fn handshake() -> (String, Vec<u8>) {
        (proto::REQUEST_SECRET.to_string(), proto::request_secret())
    }

    /// A message arrived from the server on one of our channels.
    pub fn on_net(&mut self, channel: &str, data: &[u8], hint: Option<&str>, now: Instant) {
        let Some(parsed) = proto::read(channel, data) else { return };
        let Ok(msg) = parsed else { return };
        match msg {
            proto::ServerMessage::Secret { secret, port, player, distance, voice_host, keep_alive_ms, .. } => {
                let default_host = {
                    let h = hint.unwrap_or("127.0.0.1").trim_matches('[').trim_matches(']');
                    h.rsplit_once(':').map(|(host, _)| host).unwrap_or(h)
                };
                let (host_str, target_port) = parse_voice_host(&voice_host, default_host, port);
                let resolved_ip = resolve_host(&host_str);
                let target = socket_of(&resolved_ip, target_port);
                self.server_addr = Some(target);
                self.local_player = Some(player);
                self.last_drop_reason = None;
                let mut s = Session::new(player);
                if keep_alive_ms > 0 {
                    s.keep_alive_every = std::time::Duration::from_millis(keep_alive_ms as u64);
                }
                s.set_muted(self.cfg.muted);
                s.set_max_distance(distance);
                s.on_secret(secret, target).ok();
                self.session = Some(s);
            }
            proto::ServerMessage::State(st) => self.see_state(&st, now),
            proto::ServerMessage::States(all) => {
                for st in &all {
                    self.see_state(st, now);
                }
            }
            proto::ServerMessage::RemoveState(uuid) => {
                self.player_states.remove(&uuid);
                self.last_voice_packet.remove(&uuid);
                if let Some(name) = self.name_by_uuid.remove(&uuid) {
                    self.uuid_by_name.remove(&name);
                }
                if let Some(s) = &mut self.session {
                    s.forget_position(uuid);
                }
            }
            proto::ServerMessage::AddGroup { id, name, password, persistent, hidden, kind } => {
                self.groups.insert(id, GroupInfo {
                    id,
                    name: name.clone(),
                    has_password: password,
                    persistent,
                    hidden,
                    kind,
                });
                self.notes.push(if password {
                    format!("[voice] group {} (password protected) exists", name)
                } else {
                    format!("[voice] group {} exists", name)
                });
                if self.group_id.is_none() && self.group_name.as_deref() == Some(name.as_str()) {
                    self.group_id = Some(id);
                }
            }
            proto::ServerMessage::RemoveGroup(id) => {
                self.groups.remove(&id);
                if self.group_id == Some(id) {
                    self.group_id = None;
                    self.group_name = None;
                    self.notes.push("[voice] the group was removed".into());
                }
            }
            proto::ServerMessage::JoinedGroup { group, wrong_password } => {
                if wrong_password {
                    self.notes.push("[voice] wrong group password".into());
                    self.gui_feedback = Some("Wrong group password".into());
                    self.join_password_error = Some("Wrong password!".into());
                } else {
                    self.group_id = group;
                    self.join_password_error = None;
                    if let Some(gid) = group {
                        if let Some(g) = self.groups.get(&gid) {
                            self.group_name = Some(g.name.clone());
                            self.notes.push(format!("[voice] joined group {}", g.name));
                            self.gui_feedback = Some(format!("Joined group {}", g.name));
                        } else {
                            self.notes.push("[voice] joined group".into());
                        }
                        self.screen_tab = 1;
                        self.join_password_target = None;
                        self.join_password_input.clear();
                    } else {
                        self.group_name = None;
                        self.notes.push("[voice] left group".into());
                        self.gui_feedback = Some("Left group".into());
                    }
                }
            }
        }
    }

    fn see_state(&mut self, st: &proto::PlayerState, now: Instant) {
        self.uuid_by_name.insert(st.name.clone(), st.uuid);
        self.name_by_uuid.insert(st.uuid, st.name.clone());
        self.player_states.insert(st.uuid, st.clone());
        let _ = now;
    }

    /// Check whether a player is actively speaking right now.
    pub fn is_player_talking(&self, uuid: u128, now: Instant) -> bool {
        if Some(uuid) == self.local_player {
            return self.is_talking && !self.cfg.muted;
        }
        self.last_voice_packet
            .get(&uuid)
            .map(|last| now.duration_since(*last).as_millis() < 350)
            .unwrap_or(false)
    }

    /// Query the overall voice status of a player for icons / HUD.
    pub fn player_voice_status(&self, uuid: u128, now: Instant) -> PlayerVoiceStatus {
        if Some(uuid) == self.local_player {
            if self.cfg.muted {
                return PlayerVoiceStatus::Muted;
            }
            if self.is_talking {
                return PlayerVoiceStatus::Speaking;
            }
            return PlayerVoiceStatus::Connected;
        }

        if self.is_player_talking(uuid, now) {
            return PlayerVoiceStatus::Speaking;
        }

        if let Some(st) = self.player_states.get(&uuid) {
            if st.disabled {
                return PlayerVoiceStatus::Disabled;
            }
            if st.disconnected {
                return PlayerVoiceStatus::Disconnected;
            }
            return PlayerVoiceStatus::Connected;
        }

        PlayerVoiceStatus::Disconnected
    }

    /// Open the microphone and speaker. Returns chat notes for what failed;
    /// called once per client start, devices outlive worlds.
    pub fn start_audio(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        self.refresh_devices();
        if self.mic.is_none() {
            match crate::audio_io::MicStream::open(self.cfg.input_device.as_deref()) {
                Ok(m) => self.mic = Some(m),
                Err(e) => notes.push(format!("[voice] no microphone: {}", e)),
            }
        }
        if self.speaker.is_none() {
            match crate::audio_io::Speaker::open(self.cfg.output_device.as_deref()) {
                Ok(s) => self.speaker = Some(s),
                Err(e) => notes.push(format!("[voice] no speaker: {}", e)),
            }
        }
        if self.codec.is_none() {
            match crate::audio::VoiceCodec::new() {
                Ok(c) => self.codec = Some(c),
                Err(e) => notes.push(format!("[voice] audio codec failed to start: {}", e)),
            }
        }
        notes
    }

    pub fn refresh_devices(&mut self) {
        self.cached_input_devices = crate::audio_io::list_input_devices();
        self.cached_output_devices = crate::audio_io::list_output_devices();
    }

    pub fn set_input_device(&mut self, preferred: Option<String>) -> Result<String, String> {
        let stream = crate::audio_io::MicStream::open(preferred.as_deref())?;
        let name = stream.device_name().to_string();
        self.mic = Some(stream);
        self.cfg.input_device = preferred;
        Glue::save_config("config", &self.cfg);
        Ok(name)
    }

    pub fn set_output_device(&mut self, preferred: Option<String>) -> Result<String, String> {
        let spk = crate::audio_io::Speaker::open(preferred.as_deref())?;
        let name = spk.device_name().to_string();
        self.speaker = Some(spk);
        self.cfg.output_device = preferred;
        Glue::save_config("config", &self.cfg);
        Ok(name)
    }

    pub fn active_input_device(&self) -> String {
        self.mic
            .as_ref()
            .map(|m| m.device_name().to_string())
            .unwrap_or_else(|| {
                self.cfg
                    .input_device
                    .clone()
                    .unwrap_or_else(|| "Default Microphone".into())
            })
    }

    pub fn active_output_device(&self) -> String {
        self.speaker
            .as_ref()
            .map(|s| s.device_name().to_string())
            .unwrap_or_else(|| {
                self.cfg
                    .output_device
                    .clone()
                    .unwrap_or_else(|| "Default Speaker".into())
            })
    }

    pub fn cycle_input_device(&mut self, forward: bool) -> Result<String, String> {
        self.refresh_devices();
        let mut list = vec!["Default".to_string()];
        list.extend(self.cached_input_devices.clone());
        if list.is_empty() {
            return Err("no input devices found".into());
        }
        let current = self.cfg.input_device.as_deref().unwrap_or("Default");
        let idx = list
            .iter()
            .position(|d| d.eq_ignore_ascii_case(current))
            .unwrap_or(0);
        let next_idx = if forward {
            (idx + 1) % list.len()
        } else {
            (idx + list.len() - 1) % list.len()
        };
        let target = if next_idx == 0 {
            None
        } else {
            Some(list[next_idx].clone())
        };
        self.set_input_device(target)
    }

    pub fn cycle_output_device(&mut self, forward: bool) -> Result<String, String> {
        self.refresh_devices();
        let mut list = vec!["Default".to_string()];
        list.extend(self.cached_output_devices.clone());
        if list.is_empty() {
            return Err("no output devices found".into());
        }
        let current = self.cfg.output_device.as_deref().unwrap_or("Default");
        let idx = list
            .iter()
            .position(|d| d.eq_ignore_ascii_case(current))
            .unwrap_or(0);
        let next_idx = if forward {
            (idx + 1) % list.len()
        } else {
            (idx + list.len() - 1) % list.len()
        };
        let target = if next_idx == 0 {
            None
        } else {
            Some(list[next_idx].clone())
        };
        self.set_output_device(target)
    }

    pub fn get_player_vol(&self, player: u128) -> f32 {
        self.cfg.player_volumes.get(&player).copied().unwrap_or(1.0)
    }

    pub fn set_player_vol(&mut self, player: u128, vol: f32) {
        let v = (vol.clamp(0.0, 2.0) * 100.0).round() / 100.0;
        self.cfg.player_volumes.insert(player, v);
        Glue::save_config("config", &self.cfg);
    }

    pub fn adjust_player_vol(&mut self, player: u128, delta: f32) {
        let cur = self.get_player_vol(player);
        let next = ((cur + delta) * 10.0).round() / 10.0;
        self.set_player_vol(player, next);
    }

    pub fn toggle_player_mute(&mut self, player: u128) {
        let cur = self.get_player_vol(player);
        if cur <= 0.001 {
            self.set_player_vol(player, 1.0);
        } else {
            self.set_player_vol(player, 0.0);
        }
    }

    /// The microphone side: when talking, turn captured audio into Opus
    /// packets on the session; always drain the capture so it stays fresh.
    fn pump_mic(&mut self) {
        if (self.mic.is_none() || self.speaker.is_none()) && (self.test_mode || self.mod_screen_open) {
            let _ = self.start_audio();
        }
        let Some(mic) = &mut self.mic else {
            self.is_talking = false;
            return;
        };

        let mut frames = Vec::new();
        while let Some(frame) = mic.poll_frame() {
            frames.push(frame);
        }

        let is_connected = self
            .session
            .as_ref()
            .map(|s| s.state() == crate::session::State::Connected)
            .unwrap_or(false);

        if self.cfg.muted || (!is_connected && !self.test_mode && !self.mod_screen_open) {
            self.is_talking = false;
            self.voice_hold_frames = 0;
            self.last_peak = 0;
            return;
        }

        let mut talking_this_tick = false;

        for mut frame in frames {
            let peak = frame.iter().map(|s| s.abs()).max().unwrap_or(0);
            self.last_peak = peak;

            // Apply mic capture volume / amplification
            crate::audio::mix(&mut frame, self.cfg.volume);

            let active = match self.cfg.mode {
                ActivationMode::Ptt => self.ptt_held,
                ActivationMode::Voice => {
                    if peak >= self.cfg.threshold {
                        self.voice_hold_frames = 15; // ~300ms hangover
                        true
                    } else if self.voice_hold_frames > 0 {
                        self.voice_hold_frames -= 1;
                        true
                    } else {
                        false
                    }
                }
            };

            // In test mode: hear yourself through speakers/headphones
            if self.test_mode {
                let test_active = match self.cfg.mode {
                    ActivationMode::Ptt => self.ptt_held || peak >= self.cfg.threshold || self.mod_screen_open,
                    ActivationMode::Voice => active,
                };
                if test_active {
                    talking_this_tick = true;
                    if let Some(spk) = &self.speaker {
                        let mut test_pcm = frame.clone();
                        crate::audio::mix(&mut test_pcm, self.cfg.speaker_volume);
                        spk.play(&test_pcm);
                    }
                }
            } else if active {
                talking_this_tick = true;
                if is_connected {
                    let packet = match self.codec.as_mut().map(|c| c.encode(&frame)) {
                        Some(Ok(p)) => p,
                        _ => break,
                    };
                    if let Some(s) = &mut self.session {
                        s.push_mic(&packet);
                    }
                }
            }
        }

        if self.cfg.mode == ActivationMode::Ptt {
            self.is_talking = self.ptt_held;
        } else {
            self.is_talking = talking_this_tick || self.voice_hold_frames > 0;
        }
    }

    /// The playback side: decode voice events and play them, applying the
    /// positional volume, panning, and routing directly to the per-speaker mixer channel.
    fn pump_speaker(&mut self, events: &[crate::session::Event]) {
        if (self.speaker.is_none() || self.codec.is_none()) && events.iter().any(|e| matches!(e, crate::session::Event::Voice { .. })) {
            let _ = self.start_audio();
        }
        let Some(spk) = &self.speaker else { return };

        // Clean up idle decoders older than 10 seconds
        let now = Instant::now();
        self.decoders.retain(|_, (_, last_used)| now.duration_since(*last_used) < std::time::Duration::from_secs(10));

        // Decode each incoming packet and feed it to the speaker's dedicated channel for this sender
        for e in events {
            if let crate::session::Event::Voice { sender, data, volume, pan_left, pan_right, .. } = e {
                self.last_voice_packet.insert(*sender, now);
                let decoder_entry = self.decoders.entry(*sender).or_insert_with(|| {
                    (crate::audio::VoiceDecoder::new().expect("create voice decoder"), now)
                });
                decoder_entry.1 = now;
                let decoder = &mut decoder_entry.0;

                let Ok(pcm) = decoder.decode(Some(data)) else {
                    continue;
                };

                let player_vol = self.cfg.player_volumes.get(sender).copied().unwrap_or(1.0);
                if player_vol <= 0.001 {
                    continue;
                }

                let effective_vol = *volume * self.cfg.speaker_volume * player_vol;
                let l_gain = effective_vol * pan_left;
                let r_gain = effective_vol * pan_right;

                let mut stereo = Vec::with_capacity(pcm.len() * 2);
                for sample in pcm {
                    let s = sample as f32;
                    let l = (s * l_gain).clamp(-32768.0, 32767.0) as i16;
                    let r = (s * r_gain).clamp(-32768.0, 32767.0) as i16;
                    stereo.push(l);
                    stereo.push(r);
                }

                spk.play_stereo_channel(*sender, &stereo);
            }
        }
    }

    /// One frame: drive the session, hand back the voice events it queued.
    pub fn on_frame(&mut self, _now: Instant) -> Vec<Event> {
        let mut dropped = false;
        let events = if let Some(s) = &mut self.session {
            s.tick();
            let events = s.pump();
            for e in &events {
                match e {
                    Event::Dropped(reason) => {
                        dropped = true;
                        self.last_drop_reason = Some(reason);
                    }
                    Event::Connected => {
                        self.last_drop_reason = None;
                        self.pending_sends.push((proto::CLIENT_STATE.to_string(), proto::client_state(self.cfg.muted, false)));
                        self.pending_sends.push((proto::UPDATE_STATE.to_string(), proto::update_state(false)));
                    }
                    _ => {}
                }
            }
            self.pump_speaker(&events);
            events
        } else {
            Vec::new()
        };
        if dropped {
            self.reconnect_pending = true;
            self.session = None;
        }
        self.pump_mic();
        events
    }

    /// Normalizes and compares key names between config, Horizon KeyBinds and winit codes.
    pub fn matches_key(bound: &str, physical: &str) -> bool {
        matches_key(bound, physical)
    }

    /// A key event: PTT on press and release, mute on the mute key, GUI key toggle.
    pub fn on_key(&mut self, code: &str, pressed: bool) -> Option<String> {
        if matches_key(&self.cfg.ptt_key, code) {
            self.ptt_held = pressed;
            return None;
        }
        if pressed && matches_key(&self.cfg.mute_key, code) {
            self.cfg.muted = !self.cfg.muted;
            Glue::save_config("config", &self.cfg);
            if let Some(s) = &mut self.session {
                s.set_muted(self.cfg.muted);
            }
            self.pending_sends.push((proto::CLIENT_STATE.to_string(), proto::client_state(self.cfg.muted, false)));
            self.pending_sends.push((proto::UPDATE_STATE.to_string(), proto::update_state(false)));
            return Some(if self.cfg.muted {
                "voice muted".into()
            } else {
                "voice unmuted".into()
            });
        }
        if pressed && matches_key(&self.cfg.gui_key, code) {
            if self.mod_screen_open {
                self.screen_close_requested = true;
            } else {
                self.screen_open_requested = true;
            }
            return None;
        }
        None
    }

    /// `/svc` and `/vcgroup`: returns the chat reply and any packets to send.
    pub fn on_command(&mut self, command: &str) -> (String, Vec<(String, Vec<u8>)>) {
        let mut sends = Vec::new();
        let mut args = command.split_whitespace();
        let head = args.next().unwrap_or("");
        let sub = args.next().unwrap_or("");
        let rest: Vec<&str> = args.collect();
        if head == "vcgroup" {
            return (self.group_command(sub, &rest, &mut sends), sends);
        }
        if head != "svc" {
            return (String::new(), sends);
        }
        match sub {
            "" | "gui" | "menu" | "settings" => {
                self.screen_open_requested = true;
                ("opening Voice Chat Settings...".into(), sends)
            }
            "devices" => {
                self.refresh_devices();
                let mut out = String::from("Audio Devices:\nInput (Microphones):");
                let active_in = self.active_input_device();
                if self.cached_input_devices.is_empty() {
                    out.push_str("\n  (none found - using system default)");
                } else {
                    for (i, d) in self.cached_input_devices.iter().enumerate() {
                        let mark = if d.eq_ignore_ascii_case(&active_in) {
                            " [ACTIVE]"
                        } else {
                            ""
                        };
                        out.push_str(&format!("\n  #{}: {}{}", i + 1, d, mark));
                    }
                }
                out.push_str("\nOutput (Speakers):");
                let active_out = self.active_output_device();
                if self.cached_output_devices.is_empty() {
                    out.push_str("\n  (none found - using system default)");
                } else {
                    for (i, d) in self.cached_output_devices.iter().enumerate() {
                        let mark = if d.eq_ignore_ascii_case(&active_out) {
                            " [ACTIVE]"
                        } else {
                            ""
                        };
                        out.push_str(&format!("\n  #{}: {}{}", i + 1, d, mark));
                    }
                }
                out.push_str("\nSelect: /svc mic <# or name>, /svc speaker <# or name>");
                (out, sends)
            }
            "mic" => {
                self.refresh_devices();
                if let Some(target) = rest.first() {
                    let chosen = if target.eq_ignore_ascii_case("default") || target.eq_ignore_ascii_case("0") {
                        None
                    } else if let Ok(idx) = target.parse::<usize>() {
                        if idx > 0 && idx <= self.cached_input_devices.len() {
                            Some(self.cached_input_devices[idx - 1].clone())
                        } else {
                            return (format!("invalid device #{idx}; run /svc devices"), sends);
                        }
                    } else {
                        let query = rest.join(" ");
                        Some(query)
                    };
                    match self.set_input_device(chosen) {
                        Ok(name) => (format!("microphone set to: {name}"), sends),
                        Err(e) => (format!("failed to switch microphone: {e}"), sends),
                    }
                } else {
                    (format!("current microphone: {}", self.active_input_device()), sends)
                }
            }
            "speaker" | "spk" => {
                self.refresh_devices();
                if let Some(target) = rest.first() {
                    let chosen = if target.eq_ignore_ascii_case("default") || target.eq_ignore_ascii_case("0") {
                        None
                    } else if let Ok(idx) = target.parse::<usize>() {
                        if idx > 0 && idx <= self.cached_output_devices.len() {
                            Some(self.cached_output_devices[idx - 1].clone())
                        } else {
                            return (format!("invalid device #{idx}; run /svc devices"), sends);
                        }
                    } else {
                        let query = rest.join(" ");
                        Some(query)
                    };
                    match self.set_output_device(chosen) {
                        Ok(name) => (format!("speaker set to: {name}"), sends),
                        Err(e) => (format!("failed to switch speaker: {e}"), sends),
                    }
                } else {
                    (format!("current speaker: {}", self.active_output_device()), sends)
                }
            }
            "mute" => {
                self.cfg.muted = true;
                Glue::save_config("config", &self.cfg);
                if let Some(s) = &mut self.session {
                    s.set_muted(true);
                }
                sends.push((proto::CLIENT_STATE.to_string(), proto::client_state(true, false)));
                sends.push((proto::UPDATE_STATE.to_string(), proto::update_state(false)));
                ("voice muted".into(), sends)
            }
            "unmute" => {
                self.cfg.muted = false;
                Glue::save_config("config", &self.cfg);
                if let Some(s) = &mut self.session {
                    s.set_muted(false);
                }
                sends.push((proto::CLIENT_STATE.to_string(), proto::client_state(false, false)));
                sends.push((proto::UPDATE_STATE.to_string(), proto::update_state(false)));
                ("voice unmuted".into(), sends)
            }
            "mode" => {
                let target = rest.first().copied().unwrap_or("");
                match target {
                    "ptt" | "push" => {
                        self.cfg.mode = ActivationMode::Ptt;
                        Glue::save_config("config", &self.cfg);
                        (format!("voice mode: Push-To-Talk (hold key {})", self.cfg.ptt_key), sends)
                    }
                    "voice" | "activation" | "auto" => {
                        self.cfg.mode = ActivationMode::Voice;
                        Glue::save_config("config", &self.cfg);
                        (format!("voice mode: Voice Activation (threshold: {} / ~{:.0}%)", self.cfg.threshold, (self.cfg.threshold as f32 / 327.67).clamp(0.0, 100.0)), sends)
                    }
                    _ => (
                        format!("current mode: {:?}. Usage: /svc mode ptt | /svc mode voice", self.cfg.mode),
                        sends,
                    ),
                }
            }
            "ptt" | "pttkey" => {
                if let Some(k) = rest.first() {
                    let key = if !k.starts_with("Key") && k.len() == 1 {
                        format!("Key{}", k.to_ascii_uppercase())
                    } else {
                        k.to_string()
                    };
                    self.cfg.ptt_key = key;
                    Glue::save_config("config", &self.cfg);
                    (format!("push-to-talk key set to {}", self.cfg.ptt_key), sends)
                } else {
                    (format!("push-to-talk key is {}", self.cfg.ptt_key), sends)
                }
            }
            "mutekey" => {
                if let Some(k) = rest.first() {
                    let key = if !k.starts_with("Key") && k.len() == 1 {
                        format!("Key{}", k.to_ascii_uppercase())
                    } else {
                        k.to_string()
                    };
                    self.cfg.mute_key = key;
                    Glue::save_config("config", &self.cfg);
                    (format!("mute key set to {}", self.cfg.mute_key), sends)
                } else {
                    (format!("mute key is {}", self.cfg.mute_key), sends)
                }
            }
            "guikey" => {
                if let Some(k) = rest.first() {
                    let key = if !k.starts_with("Key") && k.len() == 1 {
                        format!("Key{}", k.to_ascii_uppercase())
                    } else {
                        k.to_string()
                    };
                    self.cfg.gui_key = key;
                    Glue::save_config("config", &self.cfg);
                    (format!("GUI key set to {}", self.cfg.gui_key), sends)
                } else {
                    (format!("GUI key is {}", self.cfg.gui_key), sends)
                }
            }
            "threshold" | "sens" => {
                if let Some(arg) = rest.first() {
                    let clean = arg.trim_end_matches('%');
                    if let Ok(num) = clean.parse::<f32>() {
                        let threshold = if num <= 100.0 {
                            (num * 327.67) as i16
                        } else {
                            num.clamp(0.0, 32767.0) as i16
                        };
                        self.cfg.threshold = threshold;
                        Glue::save_config("config", &self.cfg);
                        (format!("voice threshold set to {} (~{:.0}%)", threshold, (threshold as f32 / 327.67).clamp(0.0, 100.0)), sends)
                    } else {
                        ("usage: /svc threshold <1..100%>".into(), sends)
                    }
                } else {
                    (format!("threshold is {} (~{:.0}%). Set with /svc threshold <1..100%>", self.cfg.threshold, (self.cfg.threshold as f32 / 327.67).clamp(0.0, 100.0)), sends)
                }
            }
            "test" | "loopback" => {
                self.test_mode = !self.test_mode;
                if self.test_mode {
                    ("microphone loopback test ENABLED: speak to hear yourself in headphones. Run /svc test to disable.".into(), sends)
                } else {
                    ("microphone loopback test DISABLED.".into(), sends)
                }
            }
            "hud" => {
                self.cfg.show_hud = !self.cfg.show_hud;
                Glue::save_config("config", &self.cfg);
                (format!("HUD mic display: {}", if self.cfg.show_hud { "ON" } else { "OFF" }), sends)
            }
            "volume" | "micvol" => {
                if let Some(arg) = rest.first() {
                    let clean = arg.trim_end_matches('%');
                    if let Ok(num) = clean.parse::<f32>() {
                        let vol = if num > 2.0 { num / 100.0 } else { num }.clamp(0.0, 2.0);
                        self.cfg.volume = vol;
                        Glue::save_config("config", &self.cfg);
                        (format!("microphone capture volume set to {:.0}%", vol * 100.0), sends)
                    } else {
                        ("usage: /svc micvol <0..200%>".into(), sends)
                    }
                } else {
                    (format!("microphone volume is {:.0}%", self.cfg.volume * 100.0), sends)
                }
            }
            "spkvol" | "voicevol" => {
                if let Some(arg) = rest.first() {
                    let clean = arg.trim_end_matches('%');
                    if let Ok(num) = clean.parse::<f32>() {
                        let vol = if num > 2.0 { num / 100.0 } else { num }.clamp(0.0, 2.0);
                        self.cfg.speaker_volume = vol;
                        Glue::save_config("config", &self.cfg);
                        (format!("speaker playback volume set to {:.0}%", vol * 100.0), sends)
                    } else {
                        ("usage: /svc spkvol <0..200%>".into(), sends)
                    }
                } else {
                    (format!("speaker volume is {:.0}%", self.cfg.speaker_volume * 100.0), sends)
                }
            }
            "status" => (
                {
                    let conn = match &self.session {
                        None => "disconnected".to_string(),
                        Some(_) => format!("connected to {}", self.server_addr.map(|a| a.to_string()).unwrap_or_else(|| "server".into())),
                    };
                    let mode_str = match self.cfg.mode {
                        ActivationMode::Ptt => format!("PTT (key: {}, held: {})", self.cfg.ptt_key, self.ptt_held),
                        ActivationMode::Voice => format!("Voice Activation (threshold: {} / ~{:.0}%)", self.cfg.threshold, (self.cfg.threshold as f32 / 327.67).clamp(0.0, 100.0)),
                    };
                    format!(
                        "status: {conn} | Mode: {mode_str} | Muted: {} | Speaking: {} | Test: {}\nMic: {} | Speaker: {}",
                        self.cfg.muted, self.is_talking, self.test_mode,
                        self.active_input_device(), self.active_output_device()
                    )
                },
                sends,
            ),
            _ => (
                "commands: /svc gui, /svc devices, /svc mic <#>, /svc speaker <#>, /svc mode <ptt|voice>, /svc ptt <key>, /svc threshold <1..100>, /svc test, /svc mute, /svc unmute, /svc hud, /svc status".into(),
                sends,
            ),
        }
    }

    fn group_command(&mut self, sub: &str, rest: &[&str], sends: &mut Vec<(String, Vec<u8>)>) -> String {
        match sub {
            "create" => {
                let Some(name) = rest.first() else {
                    return "usage: /vcgroup create <name> [password] [normal|open|isolated]".into();
                };
                let password = rest.get(1).copied();
                let kind = match rest.get(2).copied().map(|s| s.to_ascii_lowercase()).as_deref() {
                    Some("open") => proto::GroupKind::Open,
                    Some("isolated") => proto::GroupKind::Isolated,
                    _ => proto::GroupKind::Normal,
                };
                sends.push((
                    proto::CREATE_GROUP.to_string(),
                    proto::create_group(name, password, kind),
                ));
                self.group_name = Some(name.to_string());
                if password.is_some() {
                    format!("creating password-protected {:?} group {}", kind, name)
                } else {
                    format!("creating {:?} group {}", kind, name)
                }
            }
            "join" => {
                let Some(target) = rest.first() else {
                    return "usage: /vcgroup join <group name or id> [password]".into();
                };
                let password = rest.get(1).copied();
                let group_id = if let Ok(u) = target.parse::<u128>() {
                    Some(u)
                } else {
                    self.groups.values().find(|g| g.name.eq_ignore_ascii_case(target)).map(|g| g.id)
                };
                let Some(id) = group_id else {
                    return format!("group '{}' not found. Run /vcgroup list to see active groups.", target);
                };
                sends.push((proto::SET_GROUP.to_string(), proto::join_group(id, password)));
                format!("joining group {}...", target)
            }
            "leave" => {
                sends.push((proto::LEAVE_GROUP.to_string(), proto::leave_group()));
                self.group_id = None;
                self.group_name = None;
                "left the group".into()
            }
            "list" => {
                if self.groups.is_empty() {
                    "no groups active on server. Create one with /vcgroup create <name>".into()
                } else {
                    let mut lines = Vec::new();
                    for g in self.groups.values() {
                        let pw = if g.has_password { " [password]" } else { "" };
                        let count = self.player_states.values().filter(|s| s.group == Some(g.id)).count();
                        lines.push(format!("- {}{pw} ({} members)", g.name, count));
                    }
                    format!("active groups ({}):\n{}", self.groups.len(), lines.join("\n"))
                }
            }
            "members" => {
                let Some(gid) = self.group_id else {
                    return "you are not in any group. Join one with /vcgroup join <name>".into();
                };
                let gname = self.group_name.as_deref().unwrap_or("current");
                let mut members: Vec<String> = self.player_states.values()
                    .filter(|s| s.group == Some(gid))
                    .map(|s| s.name.clone())
                    .collect();
                members.sort();
                format!("group '{}' members ({}): {}", gname, members.len(), members.join(", "))
            }
            _ => "usage: /vcgroup create <name> [pw] [normal|open|isolated] | join <name> [pw] | leave | list | members".into(),
        }
    }

    pub fn cycle_create_group_type(&mut self) {
        self.create_group_type = match self.create_group_type {
            proto::GroupKind::Normal => proto::GroupKind::Open,
            proto::GroupKind::Open => proto::GroupKind::Isolated,
            proto::GroupKind::Isolated => proto::GroupKind::Normal,
        };
    }

    pub fn submit_create_group(&mut self) -> Option<(String, Vec<u8>)> {
        let name = self.create_group_name.trim();
        if name.is_empty() {
            self.gui_feedback = Some("Group name cannot be empty".into());
            return None;
        }
        let pw = self.create_group_password.trim();
        let pw_opt = if pw.is_empty() { None } else { Some(pw) };
        let packet = proto::create_group(name, pw_opt, self.create_group_type);
        let name_str = name.to_string();
        self.group_name = Some(name_str.clone());
        self.gui_feedback = Some(format!("Creating group {name_str}..."));
        self.screen_tab = 1;
        Some((proto::CREATE_GROUP.to_string(), packet))
    }

    pub fn submit_join_password(&mut self) -> Option<(String, Vec<u8>)> {
        let (gid, gname) = self.join_password_target.clone()?;
        let pw = self.join_password_input.trim();
        let pw_opt = if pw.is_empty() { None } else { Some(pw) };
        let packet = proto::join_group(gid, pw_opt);
        self.gui_feedback = Some(format!("Joining {gname}..."));
        Some((proto::SET_GROUP.to_string(), packet))
    }

    pub fn char_typed(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        match self.screen_tab {
            2 => {
                if self.create_group_focus == 0 {
                    if self.create_group_name.chars().count() < 16 {
                        self.create_group_name.push(c);
                    }
                } else if self.create_group_focus == 1 {
                    if self.create_group_password.chars().count() < 16 {
                        self.create_group_password.push(c);
                    }
                }
            }
            3 => {
                if self.join_password_input.chars().count() < 16 {
                    self.join_password_input.push(c);
                }
            }
            4 => {
                if self.volume_search.chars().count() < 24 {
                    self.volume_search.push(c);
                    self.volume_page = 0;
                }
            }
            _ => {}
        }
    }

    pub fn key_pressed(&mut self, key: &str, _shift: bool) -> bool {
        let is_backspace = key == "key.keyboard.backspace" || key.eq_ignore_ascii_case("backspace");
        let is_tab = key == "key.keyboard.tab" || key.eq_ignore_ascii_case("tab");
        let is_enter = key == "key.keyboard.enter" || key.eq_ignore_ascii_case("enter");
        let is_esc = key == "key.keyboard.escape" || key.eq_ignore_ascii_case("escape");

        match self.screen_tab {
            2 => {
                if is_backspace {
                    if self.create_group_focus == 0 {
                        self.create_group_name.pop();
                    } else {
                        self.create_group_password.pop();
                    }
                    return true;
                } else if is_tab {
                    self.create_group_focus = if self.create_group_focus == 0 { 1 } else { 0 };
                    return true;
                } else if is_enter {
                    if let Some((ch, body)) = self.submit_create_group() {
                        self.pending_sends.push((ch, body));
                    }
                    return true;
                } else if is_esc {
                    self.screen_tab = 1;
                    return true;
                }
            }
            3 => {
                if is_backspace {
                    self.join_password_input.pop();
                    return true;
                } else if is_enter {
                    if let Some((ch, body)) = self.submit_join_password() {
                        self.pending_sends.push((ch, body));
                    }
                    return true;
                } else if is_esc {
                    self.screen_tab = 1;
                    self.join_password_target = None;
                    self.join_password_input.clear();
                    self.join_password_error = None;
                    return true;
                }
            }
            4 => {
                if is_backspace {
                    self.volume_search.pop();
                    self.volume_page = 0;
                    return true;
                } else if is_esc {
                    self.screen_tab = 0;
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

/// Normalizes and compares key names between config, Horizon KeyBinds and winit codes.
/// Handles forms like "KeyM", "key.keyboard.m", "m", "Space", "key.keyboard.space", etc.
pub fn matches_key(bound: &str, physical: &str) -> bool {
    let norm = |s: &str| -> String {
        let t = s.trim().to_ascii_lowercase();
        let stripped = t
            .strip_prefix("key.keyboard.")
            .or_else(|| t.strip_prefix("key."))
            .or_else(|| t.strip_prefix("key"))
            .unwrap_or(&t);
        stripped.to_string()
    };
    let b = norm(bound);
    let p = norm(physical);
    !b.is_empty() && !p.is_empty() && b == p
}

fn parse_voice_host(voice_host: &str, default_host: &str, default_port: u16) -> (String, u16) {
    if voice_host.is_empty() {
        return (default_host.to_string(), default_port);
    }
    if let Ok(parsed_port) = voice_host.parse::<u16>() {
        return (default_host.to_string(), parsed_port);
    }
    let trimmed = voice_host.trim();
    if let Some((h, p_str)) = trimmed.rsplit_once(':') {
        let clean_h = h.trim_matches('[').trim_matches(']');
        let port = p_str.parse::<u16>().unwrap_or(default_port);
        let host = if clean_h.is_empty() { default_host } else { clean_h };
        (host.to_string(), port)
    } else {
        let clean_h = trimmed.trim_matches('[').trim_matches(']');
        (clean_h.to_string(), default_port)
    }
}

fn host_of(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => v4.ip().to_string(),
        SocketAddr::V6(v6) => v6.ip().to_string(),
    }
}

fn resolve_host(host: &str) -> String {
    use std::net::ToSocketAddrs;
    format!("{}:0", host)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .map(|a| host_of(a))
        .unwrap_or_else(|| host.to_string())
}

fn socket_of(host: &str, port: u16) -> SocketAddr {
    use std::net::ToSocketAddrs;
    format!("{}:{}", host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .unwrap_or_else(|| {
            ([127, 0, 0, 1], port).into()
        })
}

/// The shared state the helpers reach.
pub static STATE: OnceLock<Mutex<Glue>> = OnceLock::new();

pub fn state() -> &'static Mutex<Glue> {
    STATE.get_or_init(|| {
        let mut g = Glue::default();
        g.cfg = Glue::load_config("config");
        Mutex::new(g)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glue() -> Glue {
        Glue::default()
    }

    use std::time::Instant;

    fn secret_bytes(player: u128, port: u16, distance: f64) -> Vec<u8> {
        let mut out = Vec::new();
        // The secret reply body: fixed secret, port, player uuid, codec, mtu,
        // distance, keep-alive, groups flag, voice host, recording flag.
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&(port as i32).to_be_bytes());
        out.extend_from_slice(&player.to_be_bytes());
        out.push(0);
        out.extend_from_slice(&1024i32.to_be_bytes());
        out.extend_from_slice(&distance.to_be_bytes());
        out.extend_from_slice(&1000i32.to_be_bytes());
        out.push(1); // groups allowed
        out.push(0); // voice host: empty
        out.push(0); // recording not allowed
        out
    }

    #[test]
    fn handshake_builds_and_connects() {
        let mut g = glue();
        g.world_open();
        let (channel, body) = Glue::handshake();
        assert_eq!(channel, "voicechat:request_secret");
        assert!(!body.is_empty());
        let body = secret_bytes(7, 24454, 48.0);
        eprintln!("parse: {:?}", proto::read("voicechat:secret", &body));
        g.on_net("voicechat:secret", &body, None, Instant::now());
        assert!(g.session.is_some());
        assert_eq!(g.server_addr.unwrap().port(), 24454);
    }

    #[test]
    fn states_build_the_name_map() {
        let mut g = glue();
        let body = {
            let mut out = Vec::new();
            // A states body: count as an int, then per player disabled,
            // disconnected, uuid, name, group.
            out.extend_from_slice(&2i32.to_be_bytes());
            for (uuid, name) in [(9u128, "a"), (10, "b")] {
                out.push(0); // not disabled
                out.push(0); // not disconnected
                out.extend_from_slice(&uuid.to_be_bytes());
                out.push(name.len() as u8);
                out.extend_from_slice(name.as_bytes());
                out.push(0); // no group
            }
            out
        };
        g.on_net("voicechat:states", &body, None, Instant::now());
        assert_eq!(g.uuid_by_name.get("a"), Some(&9));
        assert_eq!(g.name_by_uuid.get(&10), Some(&"b".to_string()));
        assert_eq!(g.player_voice_status(9, Instant::now()), PlayerVoiceStatus::Connected);
        assert_eq!(g.player_voice_status(10, Instant::now()), PlayerVoiceStatus::Connected);
        assert_eq!(g.player_voice_status(999, Instant::now()), PlayerVoiceStatus::Disconnected);
    }

    #[test]
    fn player_voice_status_tracks_speaking_and_mute() {
        let mut g = glue();
        let now = Instant::now();
        g.local_player = Some(1);
        assert_eq!(g.player_voice_status(1, now), PlayerVoiceStatus::Connected);
        g.cfg.muted = true;
        assert_eq!(g.player_voice_status(1, now), PlayerVoiceStatus::Muted);
        g.cfg.muted = false;
        g.is_talking = true;
        assert_eq!(g.player_voice_status(1, now), PlayerVoiceStatus::Speaking);

        // Remote player speaking
        g.last_voice_packet.insert(2, now);
        assert_eq!(g.player_voice_status(2, now), PlayerVoiceStatus::Speaking);
    }

    #[test]
    fn groups_management_and_joined() {
        let mut g = glue();
        let now = Instant::now();
        let add_grp = {
            let mut o = proto::Out::default();
            o.uuid(42);
            o.text("Explorers".into());
            o.flag(false); // no password
            o.flag(false); // persistent
            o.flag(false); // hidden
            o.short(0);    // kind: normal
            o.done()
        };
        g.on_net("voicechat:add_group", &add_grp, None, now);
        assert_eq!(g.groups.len(), 1);
        assert!(g.groups.contains_key(&42));

        // Join group command
        let (reply, sends) = g.on_command("vcgroup join Explorers");
        assert!(reply.contains("Explorers"));
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0, proto::SET_GROUP);

        // Server answers JoinedGroup
        let joined = {
            let mut o = proto::Out::default();
            o.flag(true);
            o.uuid(42);
            o.flag(false); // no wrong password
            o.done()
        };
        g.on_net("voicechat:joined_group", &joined, None, now);
        assert_eq!(g.group_id, Some(42));
        assert_eq!(g.group_name.as_deref(), Some("Explorers"));
    }

    #[test]
    fn ptt_and_mute_toggle() {
        let mut g = glue();
        assert_eq!(g.on_key("KeyV", true), None);
        assert!(g.ptt_held);
        g.on_key("KeyV", false);
        assert!(!g.ptt_held);
        assert_eq!(g.on_key("KeyM", true), Some("voice muted".into()));
        assert!(g.cfg.muted);
    }

    #[test]
    fn commands_reply_and_send() {
        let mut g = glue();
        let (reply, _sends) = g.on_command("svc mute");
        assert_eq!(reply, "voice muted");
        assert!(g.cfg.muted);
        let (reply, sends) = g.on_command("vcgroup create den");
        assert!(reply.contains("den"));
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0, proto::CREATE_GROUP);
    }

    #[test]
    fn config_round_trip() {
        let dir = std::env::temp_dir().join(format!("svc-test-{}", std::process::id()));
        let cfg = VoiceConfig {
            ptt_key: "KeyB".into(),
            mute_key: "KeyN".into(),
            muted: true,
            volume: 0.5,
            speaker_volume: 1.2,
            mode: ActivationMode::Ptt,
            threshold: 1600,
            show_hud: true,
            input_device: Some("Realtek Audio".into()),
            output_device: Some("Headphones".into()),
            gui_key: "KeyU".into(),
            player_volumes: HashMap::new(),
        };
        Glue::save_config(dir.to_str().unwrap(), &cfg);
        let loaded = Glue::load_config(dir.to_str().unwrap());
        assert_eq!(loaded.ptt_key, "KeyB");
        assert_eq!(loaded.mute_key, "KeyN");
        assert!(loaded.muted);
        assert_eq!(loaded.volume, 0.5);
        assert_eq!(loaded.speaker_volume, 1.2);
        assert_eq!(loaded.mode, ActivationMode::Ptt);
        assert_eq!(loaded.threshold, 1600);
        assert!(loaded.show_hud);
        assert_eq!(loaded.input_device.as_deref(), Some("Realtek Audio"));
        assert_eq!(loaded.output_device.as_deref(), Some("Headphones"));
        assert_eq!(loaded.gui_key, "KeyU");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
