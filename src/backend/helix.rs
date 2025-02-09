// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{
    fs::{exists, rename},
    os::unix::fs::symlink,
    path::Path,
};

use tracing::{event, Level};

use crate::xdg;

use super::ColorScheme;

fn has_theme(theme_dir: &Path, filename: &str) -> bool {
    exists(theme_dir.join(filename)).unwrap_or_default()
}

fn update_theme_symlinks(color_scheme: ColorScheme) -> std::io::Result<()> {
    let helix_themes_dir = xdg::config_home().join("helix").join("themes");
    let default_theme = concat!(env!("CARGO_PKG_NAME"), "-default.toml");
    let theme_filename = match color_scheme {
        ColorScheme::NoPreference => default_theme,
        ColorScheme::PreferDark => concat!(env!("CARGO_PKG_NAME"), "-dark.toml"),
        ColorScheme::PreferLight => concat!(env!("CARGO_PKG_NAME"), "-light.toml"),
    };

    let theme_to_use = if has_theme(&helix_themes_dir, theme_filename) {
        theme_filename
    } else {
        event!(
            Level::DEBUG,
            "Theme {theme_filename} does not exist, falling back to {default_theme}"
        );
        default_theme
    };

    if has_theme(&helix_themes_dir, theme_to_use) {
        // Create a link at a temporary name and then rename it to -auto, to
        // replace -auto atomically; otherwise there might be a brief window
        // where -auto does not exist.
        let auto_theme_name = concat!(env!("CARGO_PKG_NAME"), "-auto.toml");
        let random_suffix = std::iter::from_fn(|| Some(fastrand::alphanumeric()))
            .take(10)
            .collect::<String>();
        let temp_link = helix_themes_dir.join(format!(".{auto_theme_name}-{random_suffix}"));
        let auto_theme_file = helix_themes_dir.join(auto_theme_name);
        event!(
            Level::DEBUG,
            "Linking {theme_filename} at {}",
            temp_link.display()
        );
        symlink(theme_to_use, &temp_link)?;
        event!(
            Level::INFO,
            "Linking {theme_filename} at {} to apply {color_scheme:?} to helix",
            auto_theme_file.display()
        );
        rename(&temp_link, auto_theme_file)
    } else {
        event!(
            Level::WARN,
            "None of {theme_filename} or {default_theme} exist in {}, not applying {color_scheme:?} to helix",
            helix_themes_dir.display()
        );
        Ok(())
    }
}

/// Apply the given [`ColorScheme`] to [Helix](https://helix-editor.com/).
///
/// This function expects three themes to exist at `$XDG_CONFIG_DIR/helix/themes`:
///
/// - `darklightd-light.toml` for [`ColorScheme::PreferLight`]
/// - `darklightd-dark.toml` for [`ColorScheme::PreferDark`]
/// - `darklightd-default.toml`  for [`ColorScheme::NoPreference`] and as fallback if either of the other themes is missing.
///
/// This function will then link the applicable variant to `darklight-auto.toml`
/// which can be used as `theme` in the main `config.toml` of Helix.
pub async fn apply_color_scheme(color_scheme: ColorScheme) -> std::io::Result<()> {
    match tokio::task::spawn_blocking(move || update_theme_symlinks(color_scheme)).await {
        Ok(result) => result,
        Err(error) => {
            // SAFETY: We can't abort synchronous code, so a join error is always a panic from the sync task.
            std::panic::resume_unwind(error.into_panic())
        }
    }
}
