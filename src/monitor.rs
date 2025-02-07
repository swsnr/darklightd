// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use futures::StreamExt;
use tokio::sync::watch;
use tracing::{event, span, Instrument, Level};

use crate::{backend::ColorScheme, portal};

async fn receive_color_scheme_changes(
    settings: portal::SettingsProxy<'_>,
    sender: watch::Sender<ColorScheme>,
) -> zbus::Result<()> {
    let mut changed_stream = settings.receive_setting_changed().await?;
    while let Some(change) = changed_stream.next().await {
        let args = change.args()?;
        if *args.namespace() == "org.freedesktop.appearance" && *args.key() == "color-scheme" {
            let raw_value = u32::try_from(args.value())?;
            let color_scheme = ColorScheme::from(raw_value);
            event!(Level::DEBUG, "org.freedesktop.appearance color-scheme changed to {raw_value} parsed as {color_scheme:?}");
            if *sender.borrow() != color_scheme && sender.send(color_scheme).is_err() {
                // If no one's listening anymore just stop receiving changes
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn monitor_color_scheme_changes(
    connection: zbus::Connection,
    sender: watch::Sender<ColorScheme>,
) -> Result<(), zbus::Error> {
    let settings = portal::SettingsProxy::builder(&connection)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    event!(Level::INFO, "Connected to settings portal, reading current color scheme from org.freedesktop.appearance color-scheme");
    let reply = settings
        .read_one("org.freedesktop.appearance", "color-scheme")
        .await?;
    let color_scheme = u32::try_from(reply)?.into();
    // We deliberately send the initial value to make the current scheme apply
    if sender.send(color_scheme).is_ok() {
        event!(Level::INFO, "Watching for color scheme changes");
        receive_color_scheme_changes(settings, sender).await
    } else {
        Ok(())
    }
}

pub fn spawn_color_scheme_monitor(
    connection: zbus::Connection,
    sender: watch::Sender<ColorScheme>,
) -> tokio::task::JoinHandle<zbus::Result<()>> {
    tokio::spawn(async move {
        monitor_color_scheme_changes(connection, sender)
            .instrument(
                span!(Level::INFO, "settings-monitor", task.id = %tokio::task::id()).or_current(),
            )
            .await
    })
}
