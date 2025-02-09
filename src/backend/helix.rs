// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{
    ffi::OsString,
    fs::{exists, rename, DirEntry, File},
    io::{ErrorKind, Read},
    os::{
        fd::AsFd,
        unix::{ffi::OsStringExt, fs::symlink},
    },
    path::{Path, PathBuf},
};

use rustix::{
    fs::{openat, readlinkat, Mode, OFlags},
    process::{pidfd_send_signal, Signal},
};
use tokio::task::JoinSet;
use tracing::{event, Level};

use crate::xdg;

use super::ColorScheme;

fn is_helix_process<F: AsFd>(process: F) -> std::io::Result<bool> {
    // Check if the executable ends with helix or if helix is somewhere in cmdline[0]
    let target = PathBuf::from(OsString::from_vec(
        readlinkat(process.as_fd(), "exe", Vec::new())?.into_bytes(),
    ));
    if target
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s == "helix")
    {
        return Ok(true);
    }

    let mut source: File =
        openat(process.as_fd(), "cmdline", OFlags::CLOEXEC, Mode::empty())?.into();
    let mut cmdline = String::new();
    source.read_to_string(&mut cmdline)?;
    if let Some(argv0) = cmdline.split('\0').next() {
        if argv0.contains("helix") {
            return Ok(true);
        }
    }

    Ok(false)
}

fn process_dentry(dentry: &DirEntry) -> std::io::Result<()> {
    let pidfd = rustix::fs::open(
        dentry.path(),
        OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    if is_helix_process(&pidfd)? {
        event!(
            Level::INFO,
            "Sending USR1 to presumed helix process {}",
            dentry.file_name().to_string_lossy()
        );
        pidfd_send_signal(pidfd, Signal::Usr1)?;
    }
    Ok(())
}

fn update_all_helix_processes() -> JoinSet<()> {
    let mut process_tasks = JoinSet::new();
    match std::fs::read_dir("/proc") {
        Err(error) => event!(Level::ERROR, "Failed to open /proc for reading: {error}"),
        Ok(dentries) => {
            for dentry in dentries.flatten() {
                process_tasks.spawn_blocking(move || {
                    if let Err(error) = process_dentry(&dentry) {
                        match error.kind() {
                            // Don't log if we've been looking at processes we
                            // don't have permission to access, process that
                            // we shortlived and vanished while we were looking
                            // at them, and other non-directory things in /proc.
                            ErrorKind::PermissionDenied
                            | ErrorKind::NotFound
                            | ErrorKind::NotADirectory => {}
                            _ => {
                                event!(
                                    Level::DEBUG,
                                    "Failed to handle dentry {}: {error}",
                                    dentry.path().display()
                                );
                            }
                        }
                    }
                });
            }
        }
    }
    process_tasks
}

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
        Err(std::io::Error::new(
            ErrorKind::NotFound,
            "No helix themes found for darklightd",
        ))
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
///
/// After successfully updating the symlink, iterate over all running proesses,
/// and send `SIGUSR1` to all processes whose executable is named `helix` or
/// whose commandline has `helix` in its first field.  This attempts to tell
/// running helix processes to reload their configuration.
pub async fn apply_color_scheme(color_scheme: ColorScheme) -> std::io::Result<()> {
    match tokio::task::spawn_blocking(move || update_theme_symlinks(color_scheme)).await {
        Ok(Ok(())) => {
            update_all_helix_processes().join_all().await;
            Ok(())
        }
        Ok(Err(error)) => {
            event!(Level::WARN, "Failed to update helix theme: {error}");
            Err(error)
        }
        Err(error) => {
            // SAFETY: We can't abort synchronous code, so a join error is always a panic from the sync task.
            std::panic::resume_unwind(error.into_panic())
        }
    }
}
