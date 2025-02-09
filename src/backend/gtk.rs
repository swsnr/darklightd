// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{io::ErrorKind, process::Stdio};

use tokio::process::Command;
use tracing::info;

use super::ColorScheme;

/// Apply the given colour scheme to Gtk.
///
/// If `color_scheme` is [`ColorScheme::PreferDark`] change the `gtk-theme`
/// key in the `org.gnome.desktop.interface` namespace to `Adwaita-dark`.
/// Otherwise reset the key to its default value.
pub async fn apply_color_scheme(color_scheme: ColorScheme) -> std::io::Result<()> {
    let mut command = Command::new("gsettings");
    if let ColorScheme::PreferDark = color_scheme {
        command.args([
            "set",
            "org.gnome.desktop.interface",
            "gtk-theme",
            "Adwaita-dark",
        ]);
    } else {
        command.args(["reset", "org.gnome.desktop.interface", "gtk-theme"]);
    }
    info!("Running {command:?} to apply color scheme {color_scheme:?} to Gtk");
    let output = command
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?
        .wait_with_output()
        .await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            ErrorKind::Other,
            format!(
                "{command:?} failed with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
        ))
    }
}
