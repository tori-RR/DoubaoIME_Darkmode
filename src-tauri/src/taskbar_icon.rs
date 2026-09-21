//! Independently managed TSF branding. Never patches the vendor's binary.
//! HKLM CTF/TIP is shared between registry views: write its native view once.
use crate::{avatar, icon_resource, safe_fs};
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};

const PROFILE: &str = r"SOFTWARE\Microsoft\CTF\TIP\{9D2B2E2B-3C93-4D2F-9D35-6EEB85F0D2B0}\LanguageProfile\0x00000804\{2B4D4B3A-4D4F-4C0A-8E66-7F771A2B9C10}";
const ROOT: &str = r"C:\Program Files\DoubaoIME\dmdm-taskbar-icon";
const JOURNAL_LIMIT: u64 = 64 * 1024;
type Handle = *mut c_void;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Request {
    #[default]
    Keep,
    Default,
    Custom {
        png: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Value {
    kind: u32,
    data: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    icon_file: Value,
    icon_index: Value,
}
impl Snapshot {
    fn validate(&self) -> Result<(), String> {
        if !matches!(self.icon_file.kind, 1 | 2)
            || self.icon_file.data.len() < 4
            || self.icon_file.data.len() > 8192
            || !self.icon_file.data.len().is_multiple_of(2)
            || self.icon_index.kind != 4
            || self.icon_index.data.len() != 4
        {
            return Err("输入法图标注册信息无效，已停止修改".into());
        }
        let raw: Vec<u16> = self
            .icon_file
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect();
        if raw.last() != Some(&0)
            || raw[..raw.len() - 1].contains(&0)
            || String::from_utf16(&raw[..raw.len() - 1]).is_err()
        {
            return Err("输入法图标路径格式无效".into());
        }
        Ok(())
    }
    fn path(&self) -> Result<PathBuf, String> {
        self.validate()?;
        let raw: Vec<u16> = self
            .icon_file
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect();
        let text = if self.icon_file.kind == 2 {
            let mut out = vec![0u16; 32768];
            let n = unsafe {
                ExpandEnvironmentStringsW(raw.as_ptr(), out.as_mut_ptr(), out.len() as u32)
            };
            if n == 0 || n as usize > out.len() {
                return Err("无法展开输入法图标路径".into());
            }
            String::from_utf16(&out[..n as usize - 1]).map_err(|e| e.to_string())?
        } else {
            String::from_utf16(&raw[..raw.len() - 1]).map_err(|e| e.to_string())?
        };
        let path = PathBuf::from(text);
        if !path.is_absolute() {
            return Err("输入法图标路径不是绝对路径".into());
        }
        Ok(path)
    }
    fn index(&self) -> i32 {
        i32::from_le_bytes(self.icon_index.data[..4].try_into().unwrap())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    job: String,
    phase: String,
    before: Snapshot,
    after: Snapshot,
}
impl Journal {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || self.job.is_empty()
            || self.job.len() > 128
            || !self
                .job
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || !matches!(
                self.phase.as_str(),
                "prepared" | "committed" | "rolled_back"
            )
        {
            return Err("任务栏图标恢复记录无效".into());
        }
        self.before.validate()?;
        self.after.validate()?;
        // Journal is protected, but also restrict the intended new target.
        if !is_default_snapshot(&self.after)? && managed_path(&self.after)?.is_none() {
            return Err("任务栏图标恢复记录的目标不受支持".into());
        }
        Ok(())
    }
}

pub struct Undo {
    pub changed: bool,
    job: Option<String>,
    _guards: Vec<File>,
    _lock: Option<File>,
}
impl Undo {
    pub fn rollback(self) -> Result<(), String> {
        if let Some(job) = &self.job {
            let mut journal = read_journal()?.ok_or("任务栏图标恢复记录丢失")?;
            if journal.job != *job || journal.phase != "prepared" {
                return Err("任务栏图标事务已变化".into());
            }
            restore_journal(&mut journal)?;
        }
        Ok(())
    }
    pub fn finish(self) -> Result<(), String> {
        if let Some(job) = &self.job {
            let mut journal = read_journal()?.ok_or("任务栏图标恢复记录丢失")?;
            if journal.job != *job
                || journal.phase != "prepared"
                || read_registry()? != journal.after
            {
                return Err("任务栏图标事务状态已变化，请恢复后重试".into());
            }
            validate_target_resource(&journal.after)?;
            journal.phase = "committed".into();
            write_journal(&journal)?;
        }
        Ok(())
    }
}

struct Key(Handle);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn open_key(write: bool) -> Result<Key, String> {
    let mut key = std::ptr::null_mut();
    let code = unsafe {
        RegOpenKeyExW(
            0x8000_0002u32 as i32 as isize as Handle,
            wide(PROFILE).as_ptr(),
            0,
            0x0100 | 1 | if write { 2 } else { 0 },
            &mut key,
        )
    };
    if code != 0 {
        return Err(format!(
            "无法{}输入法图标注册信息（{code}）",
            if write { "修改" } else { "读取" }
        ));
    }
    Ok(Key(key))
}
fn query(key: &Key, name: &str) -> Result<Value, String> {
    let mut kind = 0;
    let mut len = 8192;
    let mut data = vec![0u8; len as usize];
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            wide(name).as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            data.as_mut_ptr(),
            &mut len,
        )
    };
    if code != 0 || len as usize > data.len() {
        return Err(format!("读取图标字段 {name} 失败（{code}）"));
    }
    data.truncate(len as usize);
    Ok(Value { kind, data })
}
fn snapshot(key: &Key) -> Result<Snapshot, String> {
    let current = Snapshot {
        icon_file: query(key, "IconFile")?,
        icon_index: query(key, "IconIndex")?,
    };
    current.validate()?;
    Ok(current)
}
pub fn read_registry() -> Result<Snapshot, String> {
    snapshot(&open_key(false)?)
}
fn set_value(key: &Key, name: &str, value: &Value) -> Result<(), String> {
    let code = unsafe {
        RegSetValueExW(
            key.0,
            wide(name).as_ptr(),
            0,
            value.kind,
            value.data.as_ptr(),
            value.data.len() as u32,
        )
    };
    if code != 0 {
        return Err(format!("写入任务栏图标 {name} 失败（{code}）"));
    }
    Ok(())
}
fn set_snapshot(key: &Key, from: &Snapshot, to: &Snapshot) -> Result<(), String> {
    if snapshot(key)? != *from {
        return Err("输入法图标在授权期间发生变化，请重新安装".into());
    }
    if from.icon_file != to.icon_file {
        set_value(key, "IconFile", &to.icon_file)?;
    }
    if from.icon_index != to.icon_index {
        set_value(key, "IconIndex", &to.icon_index)?;
    }
    if snapshot(key)? != *to {
        return Err("任务栏图标写入后校验失败".into());
    }
    let code = unsafe { RegFlushKey(key.0) };
    if code != 0 {
        return Err(format!("保存任务栏图标注册信息失败（{code}）"));
    }
    Ok(())
}
fn default_path() -> Result<PathBuf, String> {
    let mut out = vec![0u16; 32768];
    let n = unsafe { GetSystemDirectoryW(out.as_mut_ptr(), out.len() as u32) };
    if n == 0 || n as usize >= out.len() {
        return Err("无法定位系统输入法图标资源".into());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(&out[..n as usize])).join("tsf-oime.dll"))
}
fn target_snapshot(path: &Path, current: &Snapshot) -> Snapshot {
    Snapshot {
        icon_file: Value {
            kind: current.icon_file.kind,
            data: wide(&path.to_string_lossy())
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
        },
        icon_index: Value {
            kind: current.icon_index.kind,
            data: 0u32.to_le_bytes().to_vec(),
        },
    }
}
fn same_icon(a: &Snapshot, b: &Snapshot) -> Result<bool, String> {
    a.validate()?;
    b.validate()?;
    Ok(a.index() == b.index()
        && a.path()?
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.path()?.to_string_lossy()))
}
fn is_default_snapshot(current: &Snapshot) -> Result<bool, String> {
    same_icon(current, &target_snapshot(&default_path()?, current))
}
fn managed_path(current: &Snapshot) -> Result<Option<PathBuf>, String> {
    let path = current.path()?;
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
        return Ok(None);
    };
    let hash = name
        .strip_prefix("icon-")
        .and_then(|v| v.strip_suffix(".dll"));
    if parent.to_string_lossy().eq_ignore_ascii_case(ROOT)
        && current.index() == 0
        && hash.is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}
pub fn is_managed() -> Result<bool, String> {
    Ok(managed_path(&read_registry()?)?.is_some())
}
pub fn is_default() -> Result<bool, String> {
    is_default_snapshot(&read_registry()?)
}
pub fn validate(request: &Request) -> Result<(), String> {
    match request {
        Request::Custom { png } => avatar::validate_stored(png),
        _ => Ok(()),
    }
}
fn desired(request: &Request, current: &Snapshot) -> Result<(Snapshot, Option<Vec<u8>>), String> {
    match request {
        Request::Keep => Ok((current.clone(), None)),
        Request::Default => Ok((target_snapshot(&default_path()?, current), None)),
        Request::Custom { png } => {
            let dll = icon_resource::build(png)?;
            let path = Path::new(ROOT).join(format!("icon-{}.dll", safe_fs::hash(&dll)));
            Ok((target_snapshot(&path, current), Some(dll)))
        }
    }
}
fn resource_matches(target: &Snapshot, bytes: Option<&[u8]>) -> bool {
    match bytes {
        None => true,
        Some(bytes) => target
            .path()
            .ok()
            .and_then(|p| safe_fs::read(&p, 4 * 1024 * 1024).ok())
            .is_some_and(|current| current == bytes),
    }
}
fn validate_target_resource(target: &Snapshot) -> Result<(), String> {
    let path = target.path()?;
    let bytes = safe_fs::read(&path, 64 * 1024 * 1024)?;
    if managed_path(target)?.is_some() {
        let expected = format!("icon-{}.dll", safe_fs::hash(&bytes));
        if path.file_name().and_then(|s| s.to_str()) != Some(expected.as_str()) {
            return Err("任务栏图标资源已变化，保留恢复记录".into());
        }
    }
    Ok(())
}
pub fn pending(request: &Request) -> Result<bool, String> {
    if matches!(request, Request::Keep) {
        return Ok(false);
    }
    let current = read_registry()?;
    let (target, data) = desired(request, &current)?;
    Ok(!same_icon(&current, &target)? || !resource_matches(&target, data.as_deref()))
}
fn journal_path() -> PathBuf {
    Path::new(ROOT).join("journal.json")
}
fn read_journal() -> Result<Option<Journal>, String> {
    let path = journal_path();
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let record: Journal = serde_json::from_slice(&safe_fs::read(&path, JOURNAL_LIMIT)?)
        .map_err(|e| format!("任务栏图标恢复记录损坏：{e}"))?;
    record.validate()?;
    Ok(Some(record))
}
fn write_journal(journal: &Journal) -> Result<(), String> {
    journal.validate()?;
    safe_fs::atomic_write(
        &journal_path(),
        &serde_json::to_vec(journal).map_err(|e| e.to_string())?,
    )
}
pub fn recovery_needed() -> Result<bool, String> {
    Ok(read_journal()?.is_some_and(|j| j.phase == "prepared"))
}
fn lock_root() -> Result<(Vec<File>, File), String> {
    let mut guards = safe_fs::guard_path(Path::new(ROOT))?;
    fs::create_dir_all(ROOT).map_err(|e| format!("创建任务栏图标资源目录失败：{e}"))?;
    guards.extend(safe_fs::guard_path(Path::new(ROOT))?);
    let lock = safe_fs::exclusive_lock(&Path::new(ROOT).join(".lock"))?;
    Ok((guards, lock))
}
// A two-value update may be interrupted between writes. Roll back only fields
// that still equal our intended value; never erase an unrelated external edit.
fn rollback_target(
    current: &Snapshot,
    before: &Snapshot,
    after: &Snapshot,
) -> Result<Snapshot, String> {
    if (current.icon_file != before.icon_file && current.icon_file != after.icon_file)
        || (current.icon_index != before.icon_index && current.icon_index != after.icon_index)
    {
        return Err("任务栏图标已被其他程序更改，保留恢复记录，请勿覆盖".into());
    }
    Ok(before.clone())
}
fn restore_journal(journal: &mut Journal) -> Result<bool, String> {
    let key = open_key(true)?;
    let current = snapshot(&key)?;
    let target = rollback_target(&current, &journal.before, &journal.after)?;
    let changed = current != target;
    set_snapshot(&key, &current, &target)?;
    journal.phase = "rolled_back".into();
    write_journal(journal)?;
    Ok(changed)
}
fn skin_committed(journal: &Journal, committed_skin_job: Option<&str>) -> bool {
    committed_skin_job == Some(journal.job.as_str())
}
pub fn recover(committed_skin_job: Option<&str>) -> Result<bool, String> {
    if !recovery_needed()? {
        return Ok(false);
    }
    let (_guards, _lock) = lock_root()?;
    let Some(mut journal) = read_journal()? else {
        return Ok(false);
    };
    if journal.phase != "prepared" {
        return Ok(false);
    }
    // The skin commit is the cross-component commit point. If the caller has
    // verified that exact job's manifest AND live hashes, complete our commit
    // rather than rolling only the branding back after a process interruption.
    if skin_committed(&journal, committed_skin_job) {
        if read_registry()? != journal.after {
            return Err("皮肤已提交但任务栏图标已被其他程序更改，保留恢复记录".into());
        }
        validate_target_resource(&journal.after)?;
        journal.phase = "committed".into();
        write_journal(&journal)?;
        return Ok(false);
    }
    restore_journal(&mut journal)
}
pub fn apply(request: &Request, expected: Option<&Snapshot>, job_id: &str) -> Result<Undo, String> {
    validate(request)?;
    if matches!(request, Request::Keep) {
        return Ok(Undo {
            changed: false,
            job: None,
            _guards: Vec::new(),
            _lock: None,
        });
    }
    let expected = expected.ok_or("缺少任务栏图标安装前快照")?;
    expected.validate()?;
    let (guards, lock) = lock_root()?;
    if recovery_needed()? {
        return Err("任务栏图标有未完成的修改，请先恢复".into());
    }
    let key = open_key(true)?;
    let before = snapshot(&key)?;
    if before != *expected {
        return Err("输入法图标在授权期间发生变化，请重新安装".into());
    }
    let (after, data) = desired(request, &before)?;
    if same_icon(&before, &after)? && resource_matches(&after, data.as_deref()) {
        return Ok(Undo {
            changed: false,
            job: None,
            _guards: guards,
            _lock: Some(lock),
        });
    }
    if let Some(data) = &data {
        let path = after.path()?;
        // Content-addressed DLLs are immutable: do not replace an existing file
        // that a running Shell could have loaded, even if it is corrupt.
        if path.try_exists().map_err(|e| e.to_string())? {
            if safe_fs::read(&path, 4 * 1024 * 1024)? != *data {
                return Err("任务栏图标资源校验失败，已停止操作".into());
            }
        } else {
            safe_fs::atomic_write(&path, data)?;
        }
    } else {
        // Validate the runtime official resource without embedding or modifying it.
        safe_fs::read(&after.path()?, 64 * 1024 * 1024)?;
    }
    let mut journal = Journal {
        schema: 1,
        job: job_id.into(),
        phase: "prepared".into(),
        before,
        after,
    };
    journal.validate()?;
    let original = Path::new(ROOT).join("original-profile.json");
    if !original.try_exists().map_err(|e| e.to_string())? {
        safe_fs::atomic_write(
            &original,
            &serde_json::to_vec(&journal.before).map_err(|e| e.to_string())?,
        )?;
    }
    write_journal(&journal)?;
    if let Err(error) = set_snapshot(&key, &journal.before, &journal.after) {
        return match restore_journal(&mut journal) {
            Ok(_) => Err(error),
            Err(rollback) => Err(format!("{error}；恢复未完成：{rollback}")),
        };
    }
    Ok(Undo {
        changed: true,
        job: Some(job_id.into()),
        _guards: guards,
        _lock: Some(lock),
    })
}

pub fn preview(request: &Request) -> Result<Option<Vec<u8>>, String> {
    if let Request::Custom { png } = request {
        validate(request)?;
        return Ok(Some(png.clone()));
    }
    let current = read_registry()?;
    let target = if matches!(request, Request::Default) {
        target_snapshot(&default_path()?, &current)
    } else {
        current
    };
    render_icon(&target.path()?, target.index()).map(Some)
}

struct Icon(Handle);
impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.0);
        }
    }
}
struct Canvas {
    dc: Handle,
    bitmap: Handle,
    old: Handle,
    pixels: *mut u8,
}
impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
#[repr(C)]
struct BitmapInfo {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bits: u16,
    compression: u32,
    image_size: u32,
    xppm: i32,
    yppm: i32,
    used: u32,
    important: u32,
    colors: [u32; 1],
}
impl Canvas {
    fn new(side: i32) -> Result<Self, String> {
        let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        if dc.is_null() {
            return Err("无法创建图标预览画布".into());
        }
        let info = BitmapInfo {
            size: 40,
            width: side,
            height: -side,
            planes: 1,
            bits: 32,
            compression: 0,
            image_size: 0,
            xppm: 0,
            yppm: 0,
            used: 0,
            important: 0,
            colors: [0],
        };
        let mut pixels = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(dc, &info, 0, &mut pixels, std::ptr::null_mut(), 0) };
        if bitmap.is_null() {
            unsafe {
                DeleteDC(dc);
            }
            return Err("无法创建图标预览位图".into());
        }
        let old = unsafe { SelectObject(dc, bitmap) };
        if old.is_null() || old as isize == -1 {
            unsafe {
                DeleteObject(bitmap);
                DeleteDC(dc);
            }
            return Err("无法选择图标预览位图".into());
        }
        Ok(Self {
            dc,
            bitmap,
            old,
            pixels: pixels.cast(),
        })
    }
    fn draw(&self, icon: &Icon, background: u8, side: i32) -> Result<Vec<u8>, String> {
        let len = side as usize * side as usize * 4;
        unsafe {
            std::ptr::write_bytes(self.pixels, background, len);
        }
        if unsafe {
            DrawIconEx(
                self.dc,
                0,
                0,
                icon.0,
                side,
                side,
                0,
                std::ptr::null_mut(),
                3,
            )
        } == 0
        {
            return Err("无法绘制图标预览".into());
        }
        unsafe {
            GdiFlush();
        }
        Ok(unsafe { std::slice::from_raw_parts(self.pixels, len) }.to_vec())
    }
}
fn render_icon(path: &Path, index: i32) -> Result<Vec<u8>, String> {
    use std::os::windows::ffi::OsStrExt;
    let _guards = safe_fs::guard_path(path)?;
    safe_fs::read(path, 64 * 1024 * 1024)?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut handle = std::ptr::null_mut();
    if unsafe { ExtractIconExW(path.as_ptr(), index, &mut handle, std::ptr::null_mut(), 1) } == 0
        || handle.is_null()
    {
        return Err("无法读取输入法图标预览".into());
    }
    let icon = Icon(handle);
    let canvas = Canvas::new(32)?;
    let black = canvas.draw(&icon, 0, 32)?;
    let white = canvas.draw(&icon, 255, 32)?;
    // Two backgrounds recover alpha for both modern alpha and legacy mask icons.
    let mut rgba = Vec::with_capacity(black.len());
    for (b, w) in black
        .as_chunks::<4>()
        .0
        .iter()
        .zip(white.as_chunks::<4>().0.iter())
    {
        let alpha = 255
            - w[0]
                .saturating_sub(b[0])
                .min(w[1].saturating_sub(b[1]))
                .min(w[2].saturating_sub(b[2]));
        for channel in [2, 1, 0] {
            rgba.push(if alpha == 0 {
                0
            } else {
                ((u32::from(b[channel]) * 255) / u32::from(alpha)).min(255) as u8
            });
        }
        rgba.push(alpha);
    }
    let image = image::RgbaImage::from_raw(32, 32, rgba).ok_or("图标预览数据无效")?;
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(
        key: Handle,
        subkey: *const u16,
        options: u32,
        access: u32,
        result: *mut Handle,
    ) -> i32;
    fn RegCloseKey(key: Handle) -> i32;
    fn RegQueryValueExW(
        key: Handle,
        name: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        len: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        key: Handle,
        name: *const u16,
        reserved: u32,
        kind: u32,
        data: *const u8,
        len: u32,
    ) -> i32;
    fn RegFlushKey(key: Handle) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn ExpandEnvironmentStringsW(source: *const u16, target: *mut u16, size: u32) -> u32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn ExtractIconExW(
        path: *const u16,
        index: i32,
        large: *mut Handle,
        small: *mut Handle,
        count: u32,
    ) -> u32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn DestroyIcon(icon: Handle) -> i32;
    fn DrawIconEx(
        dc: Handle,
        x: i32,
        y: i32,
        icon: Handle,
        width: i32,
        height: i32,
        step: u32,
        brush: Handle,
        flags: u32,
    ) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: Handle) -> Handle;
    fn CreateDIBSection(
        dc: Handle,
        info: *const BitmapInfo,
        usage: u32,
        pixels: *mut *mut c_void,
        section: Handle,
        offset: u32,
    ) -> Handle;
    fn SelectObject(dc: Handle, object: Handle) -> Handle;
    fn DeleteObject(object: Handle) -> i32;
    fn DeleteDC(dc: Handle) -> i32;
    fn GdiFlush() -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn original() -> Snapshot {
        Snapshot {
            icon_file: Value {
                kind: 1,
                data: wide(r"C:\Windows\System32\tsf-oime.dll")
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            },
            icon_index: Value {
                kind: 4,
                data: vec![0; 4],
            },
        }
    }
    #[test]
    fn request_keep_is_backward_compatible_and_noop() {
        assert!(matches!(Request::default(), Request::Keep));
        assert!(!pending(&Request::Keep).unwrap());
        assert!(validate(&Request::Custom { png: vec![1, 2, 3] }).is_err());
    }
    #[test]
    fn validate_snapshot_rejects_embedded_null_bad_types_and_bounds() {
        let mut s = original();
        s.validate().unwrap();
        s.icon_file.data.extend_from_slice(&[0, 0]);
        assert!(s.validate().is_err());
        s = original();
        s.icon_index.kind = 1;
        assert!(s.validate().is_err());
        s = original();
        s.icon_index.data.clear();
        assert!(s.validate().is_err());
    }
    #[test]
    fn rollback_accepts_partial_write_but_refuses_foreign_edits() {
        let before = original();
        let mut after = target_snapshot(
            &Path::new(ROOT).join(format!("icon-{}.dll", "a".repeat(64))),
            &before,
        );
        after.icon_index.data = 4u32.to_le_bytes().to_vec();
        let mut partial = before.clone();
        partial.icon_file = after.icon_file.clone();
        assert_eq!(rollback_target(&partial, &before, &after).unwrap(), before);
        partial.icon_index.data = 7u32.to_le_bytes().to_vec();
        assert!(rollback_target(&partial, &before, &after).is_err());
    }
    #[test]
    fn managed_scope_excludes_experiment_and_arbitrary_files() {
        let before = original();
        assert!(managed_path(&before).unwrap().is_none());
        let ours = target_snapshot(
            &Path::new(ROOT).join(format!("icon-{}.dll", "a".repeat(64))),
            &before,
        );
        assert!(managed_path(&ours).unwrap().is_some());
        let experiment = target_snapshot(
            Path::new(r"C:\Program Files\DoubaoIME\dmdm-icon-trial\brand-omega-91be33e4.dll"),
            &before,
        );
        assert!(managed_path(&experiment).unwrap().is_none());
    }
    #[test]
    fn recovery_commit_point_requires_exact_verified_skin_job() {
        let before = original();
        let after = target_snapshot(
            &Path::new(ROOT).join(format!("icon-{}.dll", "a".repeat(64))),
            &before,
        );
        let journal = Journal {
            schema: 1,
            job: "current-job".into(),
            phase: "prepared".into(),
            before,
            after,
        };
        assert!(skin_committed(&journal, Some("current-job")));
        assert!(!skin_committed(&journal, Some("previous-job")));
        assert!(!skin_committed(&journal, None));
    }
    #[test]
    fn same_default_and_custom_requests_have_identical_targets() {
        let current = original();
        let default = target_snapshot(&default_path().unwrap(), &current);
        let (again, bytes) = desired(&Request::Default, &default).unwrap();
        assert!(same_icon(&default, &again).unwrap());
        assert!(bytes.is_none());
        let mut source = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([40, 80, 120, 255]),
        ))
        .write_to(&mut source, image::ImageFormat::Png)
        .unwrap();
        let request = Request::Custom {
            png: source.into_inner(),
        };
        let (custom, bytes) = desired(&request, &current).unwrap();
        let (same, same_bytes) = desired(&request, &custom).unwrap();
        assert!(same_icon(&custom, &same).unwrap());
        assert_eq!(bytes, same_bytes);
        let dir = crate::workdir::new_scratch().unwrap();
        let path = dir.join("reused.dll");
        safe_fs::atomic_write(&path, bytes.as_ref().unwrap()).unwrap();
        let local = target_snapshot(&path, &current);
        assert!(resource_matches(&local, bytes.as_deref()));
        assert!(!resource_matches(&local, Some(b"foreign bytes")));
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn generated_icon_can_be_rendered_as_png() {
        let mut source = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([73, 147, 201, 255]),
        ))
        .write_to(&mut source, image::ImageFormat::Png)
        .unwrap();
        let dir = crate::workdir::new_scratch().unwrap();
        let path = dir.join("preview.dll");
        safe_fs::atomic_write(&path, &icon_resource::build(&source.into_inner()).unwrap()).unwrap();
        let preview = render_icon(&path, 0).unwrap();
        let image = image::load_from_memory(&preview).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (32, 32));
        assert_eq!(*image.get_pixel(16, 16), image::Rgba([73, 147, 201, 255]));
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn generated_preview_preserves_transparency() {
        let mut pixels = image::RgbaImage::new(32, 32);
        for y in 8..24 {
            for x in 8..24 {
                pixels.put_pixel(x, y, image::Rgba([80, 160, 240, 128]));
            }
        }
        let mut source = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut source, image::ImageFormat::Png)
            .unwrap();
        let dir = crate::workdir::new_scratch().unwrap();
        let path = dir.join("alpha-preview.dll");
        safe_fs::atomic_write(&path, &icon_resource::build(&source.into_inner()).unwrap()).unwrap();
        let image = image::load_from_memory(&render_icon(&path, 0).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert!(image.get_pixel(16, 16)[3].abs_diff(128) <= 1);
        for (actual, expected) in image.get_pixel(16, 16).0.iter().zip([80, 160, 240, 128]) {
            assert!(actual.abs_diff(expected) <= 2);
        }
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
