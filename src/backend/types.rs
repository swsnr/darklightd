// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

/// Color scheme preferences.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum ColorScheme {
    /// The user has no preference for light or dark mode.
    ///
    /// In Gnome this is a light mode for applications, but with a dark shell panel.
    /// Gnome also uses this value if the user did not select dark mode.
    NoPreference,
    /// The user explicitly wants dark mode.
    PreferDark,
    /// The user explicitly wants light mode.
    ///
    /// In Gnome this is light mode for applications combined with a light shell panel,
    /// provided by a built-in extension.  As of GNOME 47 users cannot explicitly
    /// select this mode in the settings UI; it's still experimental.
    PreferLight,
}

impl From<u32> for ColorScheme {
    /// Convert from an integer color scheme value.
    ///
    /// See <https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html#description>
    /// for supported values.
    fn from(value: u32) -> Self {
        match value {
            1 => Self::PreferDark,
            2 => Self::PreferLight,
            // Docs explicitly say unknown values are to be treated as no preference
            _ => Self::NoPreference,
        }
    }
}
