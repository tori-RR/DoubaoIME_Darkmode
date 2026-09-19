use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub const APP_DIR: &str = r"C:\tmp\DoubaoIME Darkmode";
pub fn review_mode() -> bool {
    std::env::args().any(|a| a == "--review-mode")
}
pub fn app_dir() -> PathBuf {
    if review_mode() {
        std::env::temp_dir().join(format!("dmdm-review-{}", std::process::id()))
    } else {
        PathBuf::from(APP_DIR)
    }
}
pub fn ensure_app_dir() -> Result<PathBuf, String> {
    let dir = app_dir();
    let _guard = crate::safe_fs::guard_path(&dir)?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建工作目录：{e}"))?;
    Ok(dir)
}
pub fn state_path() -> PathBuf {
    app_dir().join("DoubaoIME Darkmode.json")
}
pub fn user_logo_path() -> PathBuf {
    app_dir().join("logo.png")
}
pub fn new_scratch() -> Result<PathBuf, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "dmdm-run-{}-{stamp}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _guard = crate::safe_fs::guard_path(&dir)?;
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}
