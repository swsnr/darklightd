// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, clippy::all, clippy::pedantic,
    // Guard against left-over debugging output
    clippy::dbg_macro,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::unimplemented,
    clippy::use_debug,
    clippy::todo,
    clippy::undocumented_unsafe_blocks,
    // We must use tokio's APIs to exit the app.
    clippy::exit,
    // Do not carelessly ignore errors
    clippy::let_underscore_must_use,
    clippy::let_underscore_untyped,
)]
#![forbid(unsafe_code)]

use futures::StreamExt;
use logcontrol_tracing::{PrettyLogControl1LayerFactory, TracingLogControl1};
use logcontrol_zbus::ConnectionBuilderExt;
use tokio::{
    signal::{
        ctrl_c,
        unix::{signal, SignalKind},
    },
    sync::watch,
};
use tracing::{error, info, Level};
use tracing_subscriber::{layer::SubscriberExt, Registry};

mod portal;

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
enum ColorScheme {
    NoPreference,
    PreferDark,
    PreferLight,
}

impl From<u32> for ColorScheme {
    fn from(value: u32) -> Self {
        // See https://docs.flatpak.org/en/latest/portal-api-reference.html#gdbus-org.freedesktop.portal.Settings
        match value {
            1 => Self::PreferDark,
            2 => Self::PreferLight,
            // Docs explicitly say unknown values are to be treated as no preference
            _ => Self::NoPreference,
        }
    }
}

async fn monitor_color_scheme_changes(
    settings: portal::SettingsProxy<'_>,
    sender: watch::Sender<ColorScheme>,
) -> zbus::Result<()> {
    let mut changed_stream = settings.receive_setting_changed().await?;
    while let Some(change) = changed_stream.next().await {
        let args = change.args()?;
        if *args.namespace() == "org.freedesktop.appearance" && *args.key() == "color-scheme" {
            let color_scheme = ColorScheme::from(u32::try_from(args.value())?);
            info!("Notified about color scheme setting: {color_scheme:?}");
            if *sender.borrow() != color_scheme && sender.send(color_scheme).is_err() {
                // If no one's listening anymore just stop receiving changes
                return Ok(());
            }
        }
    }
    Ok(())
}

fn spawn_color_scheme_monitor(
    connection: zbus::Connection,
    sender: watch::Sender<ColorScheme>,
) -> tokio::task::JoinHandle<zbus::Result<()>> {
    tokio::spawn(async move {
        let settings = portal::SettingsProxy::builder(&connection)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?;
        info!("Connected to settings portal, reading current color scheme");
        let reply = settings
            .read_one("org.freedesktop.appearance", "color-scheme")
            .await?;
        let color_scheme = u32::try_from(reply)?.into();
        // We deliberately send the initial value to make the current scheme apply
        if sender.send(color_scheme).is_ok() {
            info!("Watching for color scheme changes");
            monitor_color_scheme_changes(settings, sender).await
        } else {
            Ok(())
        }
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup env filter for convenient log control on console
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().ok();
    // If an env filter is set with $RUST_LOG use the lowest level as default for the control part,
    // to make sure the env filter takes precedence initially.
    let default_level = if env_filter.is_some() {
        Level::TRACE
    } else if cfg!(debug_assertions) {
        // In debug builds, e.g. local testing, log more by default
        Level::DEBUG
    } else {
        Level::INFO
    };
    let (control, control_layer) =
        TracingLogControl1::new_auto(PrettyLogControl1LayerFactory, default_level)?;
    let subscriber = Registry::default().with(env_filter).with(control_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();

    tracing::info!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    let connection = zbus::connection::Builder::session()?
        .serve_log_control(logcontrol_zbus::LogControl1::new(control))?
        .name("de.swsnr.darklightd")?
        .build()
        .await?;
    info!("Connected to bus");

    let (color_scheme_tx, mut color_scheme_watch) = watch::channel(ColorScheme::NoPreference);

    let mut monitor_handle = spawn_color_scheme_monitor(connection.clone(), color_scheme_tx);
    let watch_color_scheme_handle = tokio::spawn(async move {
        while let Ok(()) = color_scheme_watch.changed().await {
            let color_scheme = *color_scheme_watch.borrow_and_update();
            info!("color scheme updated: {color_scheme:?}");
        }
    });

    let mut sigterm = signal(SignalKind::terminate())?;
    tokio::select! {
        result = ctrl_c() => {
            if let Err(error) = result {
                error!("Ctrl-C failed? {error}");
            } else {
                info!("Received SIGINT");
            }
            monitor_handle.abort();
        }
        _ = sigterm.recv() => {
            monitor_handle.abort();
        }
        result = &mut monitor_handle => {
            match result {
                Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
                _ => result??
            }
        }
    }

    connection.graceful_shutdown().await;
    // Wait until applying the last scheme change is finished
    if let Err(error) = watch_color_scheme_handle.await {
        if error.is_panic() {
            std::panic::resume_unwind(error.into_panic());
        }
    }

    Ok(())
}
