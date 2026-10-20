// SPDX-License-Identifier: GPL-3.0-or-later
// The single list of IPC command names. Included by build.rs (each command then needs an
// explicit `allow-<name>` permission), by ipc.rs (a test checks it matches `collect_commands!`)
// and by the security test (it checks the capability grants exactly these).
const COMMAND_NAMES: &[&str] = &[
    "app_info",
    "get_locale",
    "get_appearance",
    "set_theme_preference",
];
