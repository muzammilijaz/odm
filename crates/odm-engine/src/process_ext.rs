//! Spawns yt-dlp/ffmpeg without letting Windows pop up a console window.
//!
//! The desktop app runs as a GUI-subsystem (console-less) process, but
//! yt-dlp.exe/ffmpeg.exe are console-subsystem programs -- Windows allocates
//! and shows a brand new console window for them by default when a
//! console-less parent spawns one, unless the parent explicitly opts out
//! with `CREATE_NO_WINDOW`. No-op on other platforms.

use std::path::Path;
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn no_window_command(program: impl AsRef<Path>) -> Command {
    let mut cmd = Command::new(program.as_ref());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
