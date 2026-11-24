fn listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    mods.push(crate::game::native_hooks::NativeMod {
        id: "simple_voice_chat".into(),
        title: "Simple Voice Chat".into(),
        description: "Proximity voice chat for Horizon Client".into(),
        keys: vec![
            ["ptt".into(), "Push-to-Talk".into(), "key.keyboard.v".into()],
            ["mute".into(), "Mute Microphone".into(), "key.keyboard.m".into()],
            ["gui".into(), "Voice Settings Menu".into(), "key.keyboard.u".into()],
        ],
    });
}
