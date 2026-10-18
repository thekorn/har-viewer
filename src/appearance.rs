//! Bundled themes and local appearance preferences. Active colors live in GPUI's Theme.
use crate::platform;
use gpui::{App, Global, Hsla, KeyBinding, Window, actions, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, StyledExt, Theme, ThemeConfig, WindowExt,
    alert::Alert,
    button::{Button, ButtonVariants},
    radio::{Radio, RadioGroup},
    v_flex,
};
use std::{fs, io, path::PathBuf, rc::Rc};

actions!(appearance, [SelectTheme]);

struct Appearance {
    themes: Vec<Rc<ThemeConfig>>,
    path: Option<PathBuf>,
    error: Option<String>,
}

impl Global for Appearance {}

pub fn init(cx: &mut App) {
    // The component's radios are focusable but only handle pointer activation.
    // A focused theme choice must consume Enter instead of closing the dialog.
    cx.bind_keys([
        KeyBinding::new("space", SelectTheme, Some("ThemeChoice")),
        KeyBinding::new("enter", SelectTheme, Some("ThemeChoice")),
    ]);
    let themes: Vec<Rc<ThemeConfig>> = [
        include_str!("../assets/gruvbox-dark.json"),
        include_str!("../assets/gruvbox-light.json"),
    ]
    .into_iter()
    .map(|json| Rc::new(serde_json::from_str(json).expect("bundled theme must be valid")))
    .collect();
    let path = settings_path();
    let preference = path
        .as_ref()
        .ok_or_else(|| "No configuration directory is available.".to_owned())
        .and_then(|path| match fs::read_to_string(path) {
            Ok(json) => serde_json::from_str::<serde_json::Value>(&json)
                .map_err(|error| error.to_string())
                .and_then(|value| {
                    value["theme"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "Settings must contain a theme name.".into())
                })
                .map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        });
    let selected = preference.and_then(|name| match name {
        None => Ok(0),
        Some(name) => themes
            .iter()
            .position(|theme| theme.name.as_ref() == name)
            .ok_or_else(|| "The saved theme is not available.".into()),
    });
    let error = selected
        .as_ref()
        .err()
        .map(|error| format!("Could not restore appearance. Using Gruvbox Dark. {error}"));
    if let Some(error) = &error {
        eprintln!("{error}");
    }
    apply(&themes[selected.unwrap_or(0)], cx);
    cx.set_global(Appearance {
        themes,
        path,
        error,
    });
}

fn settings_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| {
                    home.join(if cfg!(target_os = "macos") {
                        "Library/Application Support"
                    } else {
                        ".config"
                    })
                })
        });
    config.map(|config| config.join("har-viewer/settings.json"))
}

fn apply(config: &Rc<ThemeConfig>, cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.apply_config(config);
    // Applying a config resets unspecified fonts to the component defaults.
    theme.font_family = platform::UI_FONT.into();
    theme.mono_font_family = platform::MONO_FONT.into();
}

fn select(index: usize, cx: &mut App) {
    let appearance = cx.global::<Appearance>();
    let config = appearance.themes[index].clone();
    let path = appearance.path.clone();
    apply(&config, cx);
    let saved = (|| -> Result<(), String> {
        let path = path.ok_or("No configuration directory is available.")?;
        fs::create_dir_all(path.parent().expect("settings has a parent"))
            .map_err(|error| error.to_string())?;
        let json = serde_json::json!({ "theme": config.name.as_ref() });
        // Replace atomically so an interrupted write cannot truncate the preference.
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&temporary, format!("{json}\n")).map_err(|error| error.to_string())?;
        if let Err(error) = fs::rename(&temporary, &path) {
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        Ok(())
    })();
    cx.global_mut::<Appearance>().error = saved
        .err()
        .map(|error| format!("Theme applied for this session, but could not be saved. {error}"));
    cx.refresh_windows();
}

pub fn open_settings(window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    window.open_dialog(cx, |dialog, _, cx| {
        let appearance = cx.global::<Appearance>();
        let selected = appearance
            .themes
            .iter()
            .position(|theme| &theme.name == cx.theme().theme_name());
        dialog
            .title("Settings")
            .width(px(480.))
            .child(
                v_flex()
                    .gap_4()
                    .child(div().font_semibold().child("Theme"))
                    .child(
                        RadioGroup::vertical("theme")
                            .children(appearance.themes.iter().enumerate().map(|(index, theme)| {
                                Radio::new(index)
                                    .label(theme.name.clone())
                                    .key_context("ThemeChoice")
                                    .on_action(move |_: &SelectTheme, _, cx| select(index, cx))
                            }))
                            .selected_index(selected)
                            .on_click(|index, _, cx| select(*index, cx)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if appearance.error.is_some() {
                                "Changes apply immediately for this session."
                            } else {
                                "Changes apply immediately and are saved for next time."
                            }),
                    )
                    .when_some(appearance.error.clone(), |this, error| {
                        this.child(
                            Alert::warning("appearance-error", error)
                                .text_color(cx.theme().foreground),
                        )
                    }),
            )
            .footer(|_, _, _, _| {
                vec![
                    Button::new("settings-done")
                        .primary()
                        .label("Done")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ]
            })
    });
}

pub fn phase_color(phase: &str, cx: &App) -> Hsla {
    let theme = cx.theme();
    match phase {
        "dns" => theme.cyan,
        "connect" => theme.red,
        "ssl" => theme.magenta,
        "send" => theme.green,
        "wait" => theme.blue,
        "receive" => theme.yellow,
        _ => theme.muted_foreground,
    }
}
