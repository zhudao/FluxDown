//! agent 任务文件打开、定位与官方桌面进程唤起。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

static DESKTOP_LAUNCHED: AtomicBool = AtomicBool::new(false);

pub fn open_task(task: &fluxdown_protocol::TaskDto) -> Result<(), PlatformError> {
    launch_path(&PathBuf::from(&task.save_dir).join(&task.file_name), false)
}

pub fn reveal_task(task: &fluxdown_protocol::TaskDto) -> Result<(), PlatformError> {
    launch_path(&PathBuf::from(&task.save_dir).join(&task.file_name), true)
}

/// 首个待确认捕获在无 UI 时只拉起一次同级桌面程序。
pub fn launch_desktop_once() -> Result<(), PlatformError> {
    if DESKTOP_LAUNCHED.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let current = std::env::current_exe()?;
    let executable = current.with_file_name(if cfg!(windows) {
        "fluxdown-desktop.exe"
    } else {
        "fluxdown-desktop"
    });
    let mut command = std::process::Command::new(executable);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    set_no_console_window(&mut command);
    command.spawn()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn launch_path(path: &Path, _reveal: bool) -> Result<(), PlatformError> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_path(path: &Path, reveal: bool) -> Result<(), PlatformError> {
    let mut command = std::process::Command::new("open");
    if reveal {
        command.arg("-R");
    }
    command.arg(path).spawn()?;
    Ok(())
}

#[cfg(windows)]
fn launch_path(path: &Path, reveal: bool) -> Result<(), PlatformError> {
    let mut command = if reveal {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(format!("/select,{}", path.display()));
        command
    } else {
        let mut command = std::process::Command::new("cmd.exe");
        command.arg("/c").arg("start").arg("").arg(path);
        command
    };
    set_no_console_window(&mut command);
    command.spawn()?;
    Ok(())
}

#[cfg(windows)]
fn set_no_console_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn set_no_console_window(_command: &mut std::process::Command) {}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("platform action failed: {0}")]
    Io(#[from] std::io::Error),
}
