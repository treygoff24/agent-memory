use crate::inbox::InboxFilter;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub action: PaletteAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    SetFilter(InboxFilter),
    OpenSearch,
    EnterRealityCheck,
    SwitchTheme(&'static str),
    ReloadTheme,
    ShowHelp,
    ReadOnly,
}

pub fn catalog() -> Vec<Command> {
    vec![
        Command { id: "filter:all", label: "filter:all", action: PaletteAction::SetFilter(InboxFilter::All) },
        Command { id: "filter:review", label: "filter:review", action: PaletteAction::SetFilter(InboxFilter::Review) },
        Command {
            id: "filter:conflicts",
            label: "filter:conflicts",
            action: PaletteAction::SetFilter(InboxFilter::Conflicts),
        },
        Command { id: "filter:recall", label: "filter:recall", action: PaletteAction::SetFilter(InboxFilter::Recall) },
        Command { id: "filter:dreams", label: "filter:dreams", action: PaletteAction::SetFilter(InboxFilter::Dreams) },
        Command { id: "filter:due", label: "filter:due", action: PaletteAction::SetFilter(InboxFilter::Due) },
        Command { id: "search", label: "search", action: PaletteAction::OpenSearch },
        Command { id: "jump:namespace", label: "jump:namespace <name>", action: PaletteAction::ReadOnly },
        Command { id: "jump:entity", label: "jump:entity <name>", action: PaletteAction::ReadOnly },
        Command { id: "reality-check:start", label: "reality-check:start", action: PaletteAction::EnterRealityCheck },
        Command {
            id: "theme:switch default-warm-dark",
            label: "theme:switch default-warm-dark",
            action: PaletteAction::SwitchTheme("default-warm-dark"),
        },
        Command {
            id: "theme:switch default-light",
            label: "theme:switch default-light",
            action: PaletteAction::SwitchTheme("default-light"),
        },
        Command {
            id: "theme:switch kanagawa",
            label: "theme:switch kanagawa",
            action: PaletteAction::SwitchTheme("kanagawa"),
        },
        Command {
            id: "theme:switch gruvbox-dark",
            label: "theme:switch gruvbox-dark",
            action: PaletteAction::SwitchTheme("gruvbox-dark"),
        },
        Command {
            id: "theme:switch catppuccin-mocha",
            label: "theme:switch catppuccin-mocha",
            action: PaletteAction::SwitchTheme("catppuccin-mocha"),
        },
        Command {
            id: "theme:switch tokyo-night",
            label: "theme:switch tokyo-night",
            action: PaletteAction::SwitchTheme("tokyo-night"),
        },
        Command { id: "theme:save-as", label: "theme:save-as <name>", action: PaletteAction::ReadOnly },
        Command { id: "theme:reload", label: "theme:reload", action: PaletteAction::ReloadTheme },
        Command { id: "device:status", label: "device:status", action: PaletteAction::ReadOnly },
        Command { id: "peer:list", label: "peer:list", action: PaletteAction::ReadOnly },
        Command { id: "dream:next-run", label: "dream:next-run", action: PaletteAction::ReadOnly },
        Command { id: "help", label: "help", action: PaletteAction::ShowHelp },
    ]
}
