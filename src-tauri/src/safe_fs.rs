use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Hold existing ancestors against rename; reject junctions and symlinks.
pub fn guard_path(path: &Path) -> Result<Vec<File>, String> {
    if !path.is_absolute() {
        return Err("必须使用绝对路径".into());
    }
    let mut guards = Vec::new();
    for part in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let meta = match fs::symlink_metadata(part) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("检查路径失败 {part:?}：{e}")),
        };
        if meta.file_attributes() & 0x400 != 0 {
            return Err(format!("拒绝重解析路径：{part:?}"));
        }
        if meta.is_dir() {
            let f = OpenOptions::new()
                .access_mode(0x80)
                .share_mode(1)
                .custom_flags(0x0220_0000)
                .open(part)
                .map_err(|e| format!("锁定目录失败 {part:?}：{e}"))?;
            if f.metadata().map_err(|e| e.to_string())?.file_attributes() & 0x400 != 0 {
                return Err("目录在检查过程中被替换".into());
            }
            guards.push(f);
        }
    }
    Ok(guards)
}

pub fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let _guards = guard_path(path)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x0020_0000)
        .open(path)
        .map_err(|e| format!("读取 {path:?} 失败：{e}"))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_attributes() & 0x400 != 0 || meta.len() > limit {
        return Err(format!("文件类型或大小不符：{path:?}"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("文件超过读取预算".into());
    }
    Ok(bytes)
}

/// Flushed same-directory replacement; failures leave the old file intact.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().ok_or("缺少父目录")?;
    let _guards = guard_path(path)?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let _new_guards = guard_path(parent)?;
    let temp = parent.join(format!(
        ".dmdm-write-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err(format!(
                "提交 {path:?} 失败：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
}

pub fn exclusive_lock(path: &Path) -> Result<File, String> {
    let _guards = guard_path(path)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .custom_flags(0x0020_0000)
        .open(path)
        .map_err(|_| "另一个窗口正在操作此安装，请稍后重试".into())
}
