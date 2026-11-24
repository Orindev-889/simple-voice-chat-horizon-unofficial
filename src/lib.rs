// Simple Voice Chat native mod for Horizon
pub use svc_client;
pub mod runtime;

use mod_api::{self, Hooks};

#[derive(Default)]
pub struct SimpleVoiceChatMod;

#[mod_api::horizon_mod]
impl Hooks for SimpleVoiceChatMod {}
