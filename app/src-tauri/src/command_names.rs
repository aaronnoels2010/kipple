// SPDX-License-Identifier: GPL-3.0-or-later
// The single list of IPC command names. Included by build.rs (each command then needs an
// explicit `allow-<name>` permission), by ipc.rs (a test checks it matches `collect_commands!`)
// and by the security test (it checks the capability grants exactly these).
const COMMAND_NAMES: &[&str] = &[
    "app_info",
    "get_locale",
    "get_appearance",
    "set_theme_preference",
    "locations_list",
    "locations_add_default",
    "locations_add_via_dialog",
    "locations_remove",
    "locations_set_enabled",
    "scan_start",
    "scan_pause",
    "scan_resume",
    "scan_cancel",
    "scan_status",
];
