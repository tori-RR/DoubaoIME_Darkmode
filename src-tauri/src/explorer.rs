//! Refresh IME branding by restarting only this session's verified shell process.
//! This module must run in the ordinary UI process, never in the elevated helper.

use std::ffi::{c_void, OsString};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const PROCESS_TERMINATE: u32 = 0x0001;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct ProcessHandle(*mut c_void);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug)]
struct ShellIdentity {
    pid: u32,
    session: u32,
    image: PathBuf,
}

fn is_expected_shell(identity: &ShellIdentity, expected: &Path, session: u32) -> bool {
    identity.pid != 0
        && identity.session == session
        // Fail closed on any different path; never accept a basename-only match.
        && identity
            .image
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy())
}

fn windows_explorer() -> Result<PathBuf, String> {
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return Err("无法确认 Windows 系统目录，未重启资源管理器".into());
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length as usize])).join("explorer.exe"))
}

fn process_session(pid: u32) -> Result<u32, String> {
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut session) } == 0 {
        return Err("无法确认资源管理器所属会话".into());
    }
    Ok(session)
}

fn shell_pid() -> Option<u32> {
    let window = unsafe { GetShellWindow() };
    if window.is_null() {
        return None;
    }
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(window, &mut pid) } == 0 || pid == 0 {
        return None;
    }
    Some(pid)
}

fn open_process(pid: u32, access: u32) -> Result<ProcessHandle, String> {
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        return Err(format!("无法访问资源管理器进程（{}）", unsafe {
            GetLastError()
        }));
    }
    Ok(ProcessHandle(handle))
}

fn identity(handle: &ProcessHandle) -> Result<ShellIdentity, String> {
    let pid = unsafe { GetProcessId(handle.0) };
    if pid == 0 {
        return Err("无法确认资源管理器进程身份".into());
    }
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle.0, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err("无法确认资源管理器程序路径".into());
    }
    Ok(ShellIdentity {
        pid,
        session: process_session(pid)?,
        image: PathBuf::from(OsString::from_wide(&buffer[..length as usize])),
    })
}

fn verified_shell(expected: &Path, session: u32) -> Result<(ProcessHandle, ShellIdentity), String> {
    let pid = shell_pid().ok_or("未找到当前桌面资源管理器，未执行重启")?;
    let handle = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)?;
    let identity = identity(&handle)?;
    if identity.pid != pid || !is_expected_shell(&identity, expected, session) {
        return Err("当前桌面进程不是本会话的 Windows 资源管理器，未执行重启".into());
    }
    if shell_pid() != Some(pid) || unsafe { WaitForSingleObject(handle.0, 0) } != WAIT_TIMEOUT {
        return Err("资源管理器状态已变化，未执行重启".into());
    }
    Ok((handle, identity))
}

fn wait_for_shell(expected: &Path, session: u32, previous_pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok((_handle, identity)) = verified_shell(expected, session) {
            if identity.pid != previous_pid {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// Restart the shell only after an applied taskbar icon actually changes.
/// No process is terminated unless its full path, session and shell ownership agree.
pub fn restart() -> Result<(), String> {
    if crate::elevate::is_admin() {
        return Err("请以普通权限打开助手；不能从管理员进程重启资源管理器".into());
    }
    if crate::workdir::review_mode() {
        return Err("检查模式不重启资源管理器".into());
    }
    let expected = windows_explorer()?;
    let session = process_session(unsafe { GetCurrentProcessId() })?;
    // Keep the first process handle alive through the operation: the verified PID
    // cannot be recycled between the read-only check and the second OpenProcess.
    let (_identity_handle, original) = verified_shell(&expected, session)?;
    let terminable = open_process(
        original.pid,
        PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE | PROCESS_TERMINATE,
    )?;
    let checked = identity(&terminable)?;
    if checked.pid != original.pid
        || !is_expected_shell(&checked, &expected, session)
        || shell_pid() != Some(original.pid)
        || unsafe { WaitForSingleObject(terminable.0, 0) } != WAIT_TIMEOUT
    {
        return Err("资源管理器状态已变化，未执行重启".into());
    }
    if unsafe { TerminateProcess(terminable.0, 0) } == 0 {
        return Err(format!("无法重启资源管理器（{}）", unsafe {
            GetLastError()
        }));
    }
    if unsafe { WaitForSingleObject(terminable.0, 5000) } != WAIT_OBJECT_0 {
        return Err("尚未确认资源管理器退出，请稍后检查任务栏".into());
    }
    // Winlogon normally restores Explorer. Give it time before launching a
    // fallback, which inherits this non-elevated process's user token.
    if wait_for_shell(&expected, session, original.pid, Duration::from_secs(6)) {
        return Ok(());
    }
    // A different/replacement shell is never overwritten by our fallback.
    if shell_pid().is_some() {
        return Err("桌面进程已改变，无法确认资源管理器恢复；请检查任务栏".into());
    }
    Command::new(&expected)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("资源管理器已退出但未能重新启动：{e}"))?;
    if wait_for_shell(&expected, session, original.pid, Duration::from_secs(10)) {
        Ok(())
    } else {
        Err("尚未确认资源管理器恢复，请手动启动 explorer.exe 或重新登录".into())
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn GetCurrentProcessId() -> u32;
    fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn GetProcessId(handle: *mut c_void) -> u32;
    fn QueryFullProcessImageNameW(
        handle: *mut c_void,
        flags: u32,
        image: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn TerminateProcess(handle: *mut c_void, exit_code: u32) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

#[link(name = "user32")]
extern "system" {
    fn GetShellWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_policy_requires_exact_image_and_current_session() {
        let expected = Path::new(r"C:\Windows\explorer.exe");
        let valid = ShellIdentity {
            pid: 123,
            session: 1,
            image: PathBuf::from(r"C:\WINDOWS\Explorer.EXE"),
        };
        assert!(is_expected_shell(&valid, expected, 1));
        assert!(!is_expected_shell(&valid, expected, 2));
        for image in [
            r"C:\Users\User\explorer.exe",
            r"C:\Windows\explorer.exe.exe",
            r"C:\Windows.old\explorer.exe",
            r"C:\Windows\System32\explorer.exe",
            r"C:\Windows\..\Other\explorer.exe",
        ] {
            assert!(!is_expected_shell(
                &ShellIdentity {
                    image: PathBuf::from(image),
                    ..valid
                },
                expected,
                1
            ));
        }
        assert!(!is_expected_shell(
            &ShellIdentity { pid: 0, ..valid },
            expected,
            1
        ));
    }

    #[test]
    #[ignore = "read-only interactive desktop check; requires a real Windows user session"]
    fn current_shell_identity_is_verified_without_restarting() {
        let expected = windows_explorer().unwrap();
        let session = process_session(unsafe { GetCurrentProcessId() }).unwrap();
        let (_handle, current) = verified_shell(&expected, session).unwrap();
        assert!(is_expected_shell(&current, &expected, session));
    }
}
