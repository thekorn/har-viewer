// Native desktop conventions shared by the viewer and its text inputs.
pub const UI_FONT: &str = if cfg!(target_os = "macos") {
    "Helvetica Neue"
} else {
    "DejaVu Sans"
};
pub const MONO_FONT: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "DejaVu Sans Mono"
};
pub const PRIMARY_MODIFIER: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};
pub const PRIMARY_LABEL: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl"
};
