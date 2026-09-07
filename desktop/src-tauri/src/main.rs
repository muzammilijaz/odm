// ODM is a GUI/tray application. Keep debug builds console-free too, because
// the autostart entry points at the debug binary during development.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    desktop_lib::run()
}
