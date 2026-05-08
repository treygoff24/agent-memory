pub static PRESETS: &[(&str, &str)] = &[
    ("default-warm-dark", include_str!("default_warm_dark.toml")),
    ("default-light", include_str!("default_light.toml")),
    ("kanagawa", include_str!("kanagawa.toml")),
    ("gruvbox-dark", include_str!("gruvbox_dark.toml")),
    ("catppuccin-mocha", include_str!("catppuccin_mocha.toml")),
    ("tokyo-night", include_str!("tokyo_night.toml")),
];

pub fn get(name: &str) -> Option<&'static str> {
    PRESETS.iter().find(|(preset, _)| *preset == name).map(|(_, body)| *body)
}
