use crate::{safe_fs, skin};
use std::ffi::OsStr;
use std::fs;
use std::os::raw::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

pub fn take_job() -> Option<Result<skin::Job, String>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--elevated") {
        return None;
    }
    Some((|| {
        if !is_admin() || args.len() != 4 {
            return Err("无效的管理员助手请求".into());
        }
        let raw = safe_fs::read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        if safe_fs::hash(&raw) != args[3] {
            return Err("授权后的请求发生变化".into());
        }
        let job: skin::Job = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        job.validate()?;
        Ok(job)
    })())
}
pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn run_elevated(job: &skin::Job) -> Result<(), String> {
    if is_admin() {
        return Err("请用普通权限打开助手；安装时会单独请求管理员授权".into());
    }
    if crate::workdir::review_mode() {
        return Err("检查模式不修改真实输入法".into());
    }
    skin::preflight(job)?;
    let receipt_root = skin::job_skin(job)?;
    let dir = crate::workdir::new_scratch()?;
    let path = dir.join("request.json");
    let raw = serde_json::to_vec(job).map_err(|e| e.to_string())?;
    safe_fs::atomic_write(&path, &raw)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let params = format!("--elevated {} {}", quote(&path), safe_fs::hash(&raw));
    crate::workdir::write_restart_pending(&job.version)?;
    let shell_was_pending = crate::workdir::shell_refresh_pending();
    let shell_may_change = crate::taskbar_icon::pending(&job.taskbar)?
        || (matches!(job.action, skin::Action::Recover) && crate::taskbar_icon::recovery_needed()?);
    // Preserve the refresh intent if this parent disappears while the helper
    // is running. Never restart Explorer merely when the application opens.
    if shell_may_change {
        crate::workdir::write_shell_refresh_pending()?;
    }
    let code = match shell_runas(&exe, &params) {
        Ok(code) => code,
        Err(err) => {
            if err.contains("已取消管理员授权") || err.contains("无法提权") {
                crate::workdir::clear_restart_pending();
                if !shell_was_pending {
                    crate::workdir::clear_shell_refresh_pending();
                }
            }
            // The request is deliberately retained when helper liveness is unknown.
            return Err(err);
        }
    };
    // Child exit is authoritative. Status text can never cause early cleanup.
    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
    let restarted = crate::ime_process::restart(Path::new(skin::IME_ROOT), &job.version);
    if restarted.is_ok() {
        crate::workdir::clear_restart_pending();
    }
    let receipt: skin::JobResult = serde_json::from_slice(&safe_fs::read(
        &receipt_root.join("dmdm_last_result.json"),
        65536,
    )?)
    .map_err(|_| "管理员助手未返回有效结果")?;
    if receipt.id != job.id {
        return Err("管理员助手结果与本次请求不一致；请检查输入法状态".into());
    }
    let shell_restarted =
        if receipt.taskbar_changed || (shell_was_pending && receipt.error.is_none()) {
            // This code runs in the ordinary UI process, never in the elevated
            // helper: launching an elevated Explorer would be a privilege bug.
            crate::workdir::write_shell_refresh_pending()?;
            let result = crate::explorer::restart()
                .map_err(|err| format!("任务栏图标已保存，但资源管理器刷新未完成：{err}"));
            if result.is_ok() {
                crate::workdir::clear_shell_refresh_pending();
            }
            result
        } else {
            if !shell_was_pending && !crate::taskbar_icon::recovery_needed().unwrap_or(true) {
                crate::workdir::clear_shell_refresh_pending();
            }
            Ok(())
        };
    if let Some(err) = receipt.error {
        return Err(match restarted {
            Ok(()) => err,
            Err(restart) => format!("{err}；{restart}"),
        });
    }
    if code != 0 {
        return Err(format!("管理员助手异常退出（{code}），请检查事务恢复提示"));
    }
    restarted.and(shell_restarted)
}
fn quote(path: &Path) -> String {
    format!("\"{}\"", path.display())
}
fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn shell_runas(exe: &Path, params: &str) -> Result<u32, String> {
    let verb = wide("runas");
    let file = wide(exe);
    let args = wide(params);
    let mut info: ShellExecuteInfoW = unsafe { std::mem::zeroed() };
    info.cb_size = std::mem::size_of::<ShellExecuteInfoW>() as u32;
    info.f_mask = 0x40;
    info.lp_verb = verb.as_ptr();
    info.lp_file = file.as_ptr();
    info.lp_parameters = args.as_ptr();
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let err = unsafe { GetLastError() };
        return Err(if err == 1223 {
            "已取消管理员授权".into()
        } else {
            format!("无法提权（{err}）")
        });
    }
    if info.h_process.is_null() {
        return Err("无法取得管理员助手进程句柄，暂存请求已保留".into());
    }
    // Blocking worker, not the UI thread. Never return or delete input while child runs.
    let wait = unsafe { WaitForSingleObject(info.h_process, u32::MAX) };
    if wait != 0 {
        unsafe { CloseHandle(info.h_process) };
        return Err("无法确认管理员助手已退出，暂存请求已保留".into());
    }
    let mut code = 1;
    let ok = unsafe { GetExitCodeProcess(info.h_process, &mut code) };
    unsafe { CloseHandle(info.h_process) };
    if ok == 0 {
        return Err("无法读取管理员助手退出状态".into());
    }
    Ok(code)
}
#[repr(C)]
struct ShellExecuteInfoW {
    cb_size: u32,
    f_mask: u32,
    hwnd: *mut c_void,
    lp_verb: *const u16,
    lp_file: *const u16,
    lp_parameters: *const u16,
    lp_directory: *const u16,
    n_show: i32,
    h_inst_app: *mut c_void,
    lp_id_list: *mut c_void,
    lp_class: *const u16,
    hkey_class: *mut c_void,
    dw_hot_key: u32,
    h_icon: *mut c_void,
    h_process: *mut c_void,
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
    fn IsUserAnAdmin() -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
    fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetExitCodeProcess(handle: *mut c_void, code: *mut u32) -> i32;
}
