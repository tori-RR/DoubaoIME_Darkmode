use serde_json::{json, Value};
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

const PIPE: &str = r"\\.\pipe\DoubaoIme\settings-rpc";
const MAX_FRAME: usize = 4 * 1024 * 1024;
static LOCK: Mutex<()> = Mutex::new(());
static SEQ: AtomicU64 = AtomicU64::new(1);
type Handle = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: Handle,
}
struct Event(Handle);
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn transfer(file: &File, bytes: &mut [u8], write: bool, deadline: Instant) -> Result<(), String> {
    let h = file.as_raw_handle();
    let event = Event(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) });
    if event.0.is_null() {
        return Err("创建管道事件失败".into());
    }
    let mut used = 0;
    while used < bytes.len() {
        if Instant::now() >= deadline {
            return Err("输入法请求超时".into());
        }
        unsafe { ResetEvent(event.0) };
        let mut ov = Overlapped {
            event: event.0,
            ..Default::default()
        };
        let mut count = 0;
        let remaining = &mut bytes[used..];
        let ok = unsafe {
            if write {
                WriteFile(
                    h,
                    remaining.as_ptr().cast(),
                    remaining.len() as u32,
                    std::ptr::null_mut(),
                    &mut ov,
                )
            } else {
                ReadFile(
                    h,
                    remaining.as_mut_ptr().cast(),
                    remaining.len() as u32,
                    std::ptr::null_mut(),
                    &mut ov,
                )
            }
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err != 997 {
                return Err(format!("管道 I/O 失败（{err}）"));
            }
            let ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(u32::MAX as u128) as u32;
            if unsafe { WaitForSingleObject(event.0, ms) } != 0 {
                unsafe {
                    CancelIoEx(h, &ov);
                    // The OVERLAPPED and buffer remain alive until cancellation completes.
                    GetOverlappedResult(h, &ov, &mut count, 1);
                }
                return Err("输入法请求超时，已取消管道操作".into());
            }
        }
        if unsafe { GetOverlappedResult(h, &ov, &mut count, 1) } == 0 {
            return Err(format!("管道 I/O 失败（{}）", unsafe {
                GetLastError()
            }));
        }
        if count == 0 {
            return Err("输入法提前关闭了连接".into());
        }
        used += count as usize;
    }
    Ok(())
}

fn exchange(
    pipe: &File,
    method: &str,
    payload: Option<Value>,
    deadline: Instant,
) -> Result<Value, String> {
    let id = format!(
        "dmdm-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let mut req = json!({"requestId":id,"method":method});
    if let Some(p) = payload {
        req["payload"] = p;
    }
    let mut body = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
    let mut header = (body.len() as u32).to_le_bytes();
    transfer(pipe, &mut header, true, deadline)?;
    transfer(pipe, &mut body, true, deadline)?;
    transfer(pipe, &mut header, false, deadline)?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(format!("输入法回包长度异常：{len}"));
    }
    let mut response = vec![0; len];
    transfer(pipe, &mut response, false, deadline)?;
    let response: Value =
        serde_json::from_slice(&response).map_err(|e| format!("回包不是 JSON：{e}"))?;
    if response.get("requestId").and_then(Value::as_str) != Some(&id) {
        return Err("输入法回包 requestId 不匹配".into());
    }
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(format!(
            "输入法拒绝了请求：{}",
            response.get("error").unwrap_or(&Value::Null)
        ));
    }
    Ok(response)
}
fn call(method: &str, payload: Option<Value>) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(6);
    let pipe = loop {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0x4000_0000)
            .open(PIPE)
        {
            Ok(f) => break f,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(80)),
            Err(_) => return Err("连不上输入法，确认豆包输入法正在运行".into()),
        }
    };
    exchange(&pipe, method, payload, deadline)
}
fn read_toolbar() -> Result<bool, String> {
    call("settings.get", None)?
        .pointer("/payload/snapshot/status_bar/enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| "读不到工具栏开关".into())
}
pub fn toolbar_enabled() -> Result<bool, String> {
    let _lock = LOCK.lock().map_err(|_| "设置连接状态异常")?;
    read_toolbar()
}
pub fn toggle_toolbar() -> Result<bool, String> {
    let _lock = LOCK.lock().map_err(|_| "设置连接状态异常")?;
    let next = !read_toolbar()?;
    call(
        "settings.update",
        Some(json!({"patch":{"status_bar":{"enabled":next}}})),
    )?;
    let actual = read_toolbar()?;
    if actual != next {
        return Err("输入法未接受工具栏设置，已重新读取实际状态".into());
    }
    Ok(actual)
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn ReadFile(h: Handle, buf: *mut c_void, len: u32, read: *mut u32, ov: *mut Overlapped) -> i32;
    fn WriteFile(
        h: Handle,
        buf: *const c_void,
        len: u32,
        written: *mut u32,
        ov: *mut Overlapped,
    ) -> i32;
    fn GetOverlappedResult(h: Handle, ov: *const Overlapped, count: *mut u32, wait: i32) -> i32;
    fn CancelIoEx(h: Handle, ov: *const Overlapped) -> i32;
    fn CreateEventW(attr: *const c_void, manual: i32, initial: i32, name: *const u16) -> Handle;
    fn ResetEvent(h: Handle) -> i32;
    fn CloseHandle(h: Handle) -> i32;
    fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    fn GetLastError() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            mode: u32,
            pipe_mode: u32,
            max: u32,
            out_size: u32,
            in_size: u32,
            timeout: u32,
            attr: *const c_void,
        ) -> Handle;
    }
    fn pair() -> (File, File) {
        let name = format!(
            r"\\.\pipe\dmdm-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let w: Vec<u16> = std::ffi::OsStr::new(&name)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let server =
            unsafe { CreateNamedPipeW(w.as_ptr(), 3, 0, 1, 65536, 65536, 0, std::ptr::null()) };
        assert_ne!(server as isize, -1);
        let client = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0x4000_0000)
            .open(name)
            .unwrap();
        (unsafe { File::from_raw_handle(server) }, client)
    }
    #[test]
    fn timeout_cancels_read_and_can_be_repeated() {
        for _ in 0..8 {
            let (server, client) = pair();
            let start = Instant::now();
            assert!(transfer(
                &client,
                &mut [0; 4],
                false,
                start + Duration::from_millis(30)
            )
            .is_err());
            assert!(start.elapsed() < Duration::from_secs(1));
            drop((server, client));
        }
    }
    #[test]
    fn half_frame_read_is_cancelled() {
        let (mut server, client) = pair();
        server.write_all(&[1, 2]).unwrap();
        assert!(transfer(
            &client,
            &mut [0; 4],
            false,
            Instant::now() + Duration::from_millis(30)
        )
        .is_err());
    }
    #[test]
    fn exchange_rejects_wrong_request_id() {
        let (mut server, client) = pair();
        let handle = std::thread::spawn(move || {
            let mut head = [0; 4];
            server.read_exact(&mut head).unwrap();
            let mut body = vec![0; u32::from_le_bytes(head) as usize];
            server.read_exact(&mut body).unwrap();
            let response = br#"{"ok":true,"requestId":"wrong"}"#;
            server
                .write_all(&(response.len() as u32).to_le_bytes())
                .unwrap();
            server.write_all(response).unwrap();
            std::thread::sleep(Duration::from_millis(50));
        });
        assert!(exchange(
            &client,
            "settings.get",
            None,
            Instant::now() + Duration::from_secs(1)
        )
        .unwrap_err()
        .contains("requestId"));
        handle.join().unwrap();
    }
}
