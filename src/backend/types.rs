// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum ColorScheme {
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
