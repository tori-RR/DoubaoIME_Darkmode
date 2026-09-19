#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod avatar;
mod elevate;
mod fonts;
mod ime_process;
mod ime_rpc;
mod kaomoji;
mod safe_fs;
mod skin;
mod theme;
mod transaction;
mod workdir;

use fonts::{list_installed, FontChoice, FontLists};
use kaomoji::get_kaomoji_groups;
use serde::{Deserialize, Serialize};
use skin::official_status_wnd;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static INSTALL_LOCK: Mutex<()> = Mutex::new(());
use theme::{
    clamp_glass_opacity, dark_colors, generate_toolbar_preview, glass_enabled, light_colors,
    mix_colors, ThemeColors, ToolbarPreview, GLASS_OPACITY_DEFAULT,
};

fn is_preset_theme(id: &str) -> bool {
    matches!(id, "dark" | "light" | "mix")
}

fn is_custom_theme_id(id: &str) -> bool {
    id.starts_with("自定义") && id.len() > "自定义".len()
}

fn known_theme(id: &str, customs: &[NamedTheme]) -> bool {
    is_preset_theme(id) || customs.iter().any(|theme| theme.id == id)
}

fn migrate_theme_id(id: &str) -> String {
    if id == "glass" {
        "dark".into()
    } else {
        id.into()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NamedTheme {
    id: String,
    #[serde(flatten)]
    colors: ThemeColors,
}

#[derive(Clone, Serialize, Deserialize)]
struct UiState {
    theme_id: String,
    custom: ThemeColors,
    #[serde(default)]
    custom_themes: Vec<NamedTheme>,
    #[serde(default)]
    fonts: FontChoice,
    #[serde(default = "default_glass_opacity")]
    glass_opacity: u8,
}

fn default_glass_opacity() -> u8 {
    GLASS_OPACITY_DEFAULT
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            theme_id: "dark".into(),
            custom: dark_colors(),
            custom_themes: Vec::new(),
            fonts: FontChoice::default(),
            glass_opacity: GLASS_OPACITY_DEFAULT,
        }
    }
}

#[derive(Serialize)]
struct AppStatus {
    ime_installed: bool,
    ime_version: Option<String>,
    verified_ime_version: String,
    verified_ime_versions: Vec<String>,
    ime_structure_matches: bool,
    ime_structure_detail: String,
    ime_compatible: bool,
    skin_applied: bool,
    installed_label: String,
    theme_id: String,
    colors: ThemeColors,
    custom: ThemeColors,
    custom_themes: Vec<NamedTheme>,
    fonts: FontChoice,
    glass_opacity: u8,
    has_custom_logo: bool,
    logo_revision: String,
    can_restore: bool,
    recovery_needed: bool,
    review_mode: bool,
    warning: String,
    app_version: String,
    workdir: String,
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn state_path() -> PathBuf {
    workdir::state_path()
}

fn old_appdata_dir() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into()))
        .join("DoubaoIME_Darkmode")
}

fn migrate_legacy_state() {
    if workdir::review_mode() || state_path().exists() {
        return;
    }
    if workdir::ensure_app_dir().is_err() {
        return;
    }
    let Ok(_lock) = state_lock() else {
        return;
    };
    if state_path().exists() {
        return;
    }
    for src in [
        exe_dir().join("DoubaoIME Darkmode.json"),
        old_appdata_dir().join("ui-state.json"),
    ] {
        if let Ok(bytes) = safe_fs::read(&src, 1024 * 1024) {
            if serde_json::from_slice::<UiState>(&bytes).is_ok() {
                let _ = safe_fs::atomic_write(&state_path(), &bytes);
                // Preserve the legacy directory, even when migration succeeds.
                break;
            }
        }
    }
}
fn state_lock() -> Result<std::fs::File, String> {
    let dir = workdir::ensure_app_dir()?;
    safe_fs::exclusive_lock(&dir.join(".state.lock"))
}
fn load_user_logo() -> Option<Vec<u8>> {
    let bytes = safe_fs::read(&workdir::user_logo_path(), avatar::MAX_UPLOAD_BYTES as u64).ok()?;
    avatar::square_pad_png(&bytes).ok()
}

fn load_state() -> UiState {
    let path = state_path();
    let mut state: UiState = safe_fs::read(&path, 1024 * 1024)
        .ok()
        .and_then(|s| String::from_utf8(s).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    state.theme_id = migrate_theme_id(&state.theme_id);
    state.glass_opacity = clamp_glass_opacity(state.glass_opacity);
    if state.theme_id == "custom" {
        if !state
            .custom_themes
            .iter()
            .any(|theme| theme.id == "自定义1")
        {
            state.custom_themes.insert(
                0,
                NamedTheme {
                    id: "自定义1".into(),
                    colors: state.custom.clone(),
                },
            );
        }
        state.theme_id = "自定义1".into();
    }
    if !known_theme(&state.theme_id, &state.custom_themes) {
        state.theme_id = "dark".into();
    }
    state
}

fn save_state(state: &UiState) -> Result<(), String> {
    let path = state_path();
    if path.exists() {
        let old = safe_fs::read(&path, 1024 * 1024)?;
        serde_json::from_slice::<UiState>(&old)
            .map_err(|_| "配置损坏，已保留原文件；请先备份并修复配置")?;
        safe_fs::atomic_write(&path.with_extension("json.bak"), &old)?;
    }
    safe_fs::atomic_write(
        &path,
        &serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?,
    )
}

fn colors_for(state: &UiState) -> ThemeColors {
    match state.theme_id.as_str() {
        "light" => light_colors(),
        "mix" => mix_colors(),
        "dark" => dark_colors(),
        id => state
            .custom_themes
            .iter()
            .find(|theme| theme.id == id)
            .map(|theme| theme.colors.clone())
            .unwrap_or_else(dark_colors),
    }
}

fn next_custom_id(themes: &[NamedTheme]) -> String {
    let used: HashSet<u32> = themes
        .iter()
        .filter_map(|theme| theme.id.strip_prefix("自定义")?.parse().ok())
        .collect();
    let mut n = 1u32;
    while used.contains(&n) {
        n += 1;
    }
    format!("自定义{n}")
}

fn current_status() -> AppStatus {
    let state = load_state();
    let target = skin::detect();
    let (structure_matches, structure_detail) = match &target {
        Ok(target) => match skin::inspect_structure(&target.skin) {
            Ok(()) => (
                true,
                format!(
                    "安装结构一致（{} 个必需文件）",
                    skin::official_hashes().len()
                ),
            ),
            Err(err) => (false, err),
        },
        Err(err) => (false, format!("无法检测安装结构：{err}")),
    };
    let mut warning = String::new();
    let (installed, version, compatible, applied, label, can_restore, recovery) = match target {
        Ok(target) => {
            let inspected = (|| -> Result<_, String> {
                let store = skin::store_for(&target)?;
                let recovery = store.recovery_needed()?;
                if recovery {
                    return Ok((false, true, "操作中断，点击恢复".into(), false, true));
                }
                let originals = store.originals()?;
                let hashes = store.live_hashes()?;
                let applied = hashes != transaction::Store::hashes(&originals);
                let label = if let Some(m) = store.manifest()? {
                    if hashes != m.applied {
                        return Err("皮肤被其他程序修改，请检查备份后再操作".into());
                    }
                    m.label
                } else if applied {
                    "已安装（旧版）".into()
                } else {
                    "官方浅色".into()
                };
                Ok((true, applied, label, store.has_backup(), false))
            })();
            match inspected {
                Ok((ok, applied, label, restore, recovery)) => (
                    true,
                    Some(target.version),
                    ok,
                    applied,
                    label,
                    restore,
                    recovery,
                ),
                Err(err) => {
                    warning = err;
                    (
                        true,
                        Some(target.version),
                        false,
                        false,
                        "需要检查".into(),
                        false,
                        false,
                    )
                }
            }
        }
        Err(err) => {
            warning = err;
            (
                Path::new(skin::IME_ROOT).is_dir(),
                None,
                false,
                false,
                "未核验".into(),
                false,
                false,
            )
        }
    };
    let logo = load_user_logo();
    if workdir::user_logo_path().exists() && logo.is_none() {
        warning = "头像无法读取，请重新选择图片；原文件已保留".into();
    }
    if state_path().exists()
        && safe_fs::read(&state_path(), 1024 * 1024)
            .ok()
            .and_then(|b| serde_json::from_slice::<UiState>(&b).ok())
            .is_none()
    {
        warning = "配置文件损坏，原文件已保留；保存已暂停".into();
    }
    AppStatus {
        ime_installed: installed,
        ime_version: version,
        verified_ime_version: skin::IME_VERSION.into(),
        verified_ime_versions: skin::VERIFIED_IME_VERSIONS
            .iter()
            .map(|version| (*version).into())
            .collect(),
        ime_structure_matches: structure_matches,
        ime_structure_detail: structure_detail,
        ime_compatible: compatible,
        skin_applied: applied,
        installed_label: label,
        colors: colors_for(&state),
        theme_id: state.theme_id,
        custom: state.custom,
        custom_themes: state.custom_themes,
        fonts: state.fonts,
        glass_opacity: clamp_glass_opacity(state.glass_opacity),
        has_custom_logo: logo.is_some(),
        logo_revision: logo.as_ref().map(|b| safe_fs::hash(b)).unwrap_or_default(),
        can_restore,
        recovery_needed: recovery,
        review_mode: workdir::review_mode(),
        warning,
        app_version: env!("CARGO_PKG_VERSION").into(),
        workdir: workdir::app_dir().display().to_string(),
    }
}
fn prepare_job(action: skin::Action, colors: Option<ThemeColors>) -> Result<skin::Job, String> {
    let target = skin::detect()?;
    let store = skin::store_for(&target)?;
    let action = if store.recovery_needed()? {
        skin::Action::Recover
    } else {
        action
    };
    // Restoration must not depend on a usable custom palette, font or avatar.
    let applying = matches!(action, skin::Action::Apply);
    let state = if applying {
        load_state()
    } else {
        UiState::default()
    };
    let palette = if applying {
        colors.unwrap_or_else(|| colors_for(&state)).normalize()?
    } else {
        dark_colors()
    };
    let logo = if applying { load_user_logo() } else { None };
    if applying && workdir::user_logo_path().exists() && logo.is_none() {
        return Err("头像无效，请重新导入".into());
    }
    let label = if palette == dark_colors() {
        "Dark".into()
    } else if palette == light_colors() {
        "Light".into()
    } else if palette == mix_colors() {
        "Midlight".into()
    } else {
        format!("自定义 {}", palette.background)
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let job = skin::Job {
        action,
        version: target.version,
        expected: store.live_hashes()?,
        colors: palette,
        fonts: state.fonts,
        opacity: state.glass_opacity,
        light: state.theme_id == "light",
        logo,
        label: if state.glass_opacity < 100 {
            format!("{label} {}%", state.glass_opacity)
        } else {
            label
        },
        id: format!("{}-{stamp}", std::process::id()),
    };
    skin::preflight(&job)?;
    Ok(job)
}
fn upload_bytes(body: &tauri::ipc::InvokeBody) -> Result<Vec<u8>, String> {
    match body {
        tauri::ipc::InvokeBody::Raw(bytes) => {
            if bytes.len() > avatar::MAX_UPLOAD_BYTES {
                return Err("图片太大（上限 8MB）".into());
            }
            Ok(bytes.clone())
        }
        // Tauri should transport a top-level ArrayBuffer as Raw. Accept its
        // JSON byte-array fallback too, so a WebView transport quirk cannot
        // disable image upload. All normal size and decoder checks still run.
        tauri::ipc::InvokeBody::Json(value) => {
            let values = value
                .as_array()
                .ok_or_else(|| "需要二进制图片".to_string())?;
            if values.len() > avatar::MAX_UPLOAD_BYTES {
                return Err("图片太大（上限 8MB）".into());
            }
            Ok(values
                .iter()
                .map(|value| {
                    value
                        .as_u64()
                        .filter(|value| *value <= u8::MAX as u64)
                        .map(|value| value as u8)
                        .ok_or_else(|| "图片数据无效".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?)
        }
    }
}

#[tauri::command]
async fn import_logo(request: tauri::ipc::Request<'_>) -> Result<AppStatus, String> {
    let bytes = upload_bytes(request.body())?;
    tauri::async_runtime::spawn_blocking(move || {
        let png = avatar::square_pad_png(&bytes)?;
        let _lock = state_lock()?;
        safe_fs::atomic_write(&workdir::user_logo_path(), &png)?;
        Ok(current_status())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn install(colors: Option<ThemeColors>) -> Result<AppStatus, String> {
    perform(skin::Action::Apply, colors).await
}
async fn perform(action: skin::Action, colors: Option<ThemeColors>) -> Result<AppStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = INSTALL_LOCK.try_lock().map_err(|_| "另一次操作尚未完成")?;
        let _state = state_lock()?;
        // Backups intentionally survive uninstall. Their presence alone must
        // never trigger another privileged restore when the skin is original.
        if matches!(action, skin::Action::Restore) {
            let status = current_status();
            if !status.skin_applied
                && !status.recovery_needed
                && (status.ime_compatible || !status.ime_installed)
            {
                return Ok(status);
            }
        }
        let job = prepare_job(action, colors)?;
        elevate::run_elevated(&job)?;
        Ok(current_status())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn uninstall() -> Result<AppStatus, String> {
    perform(skin::Action::Restore, None).await
}

#[tauri::command]
fn get_status() -> AppStatus {
    current_status()
}

#[tauri::command]
fn set_theme_id(theme_id: String) -> Result<AppStatus, String> {
    let _state_lock = state_lock()?;
    let theme_id = migrate_theme_id(&theme_id);
    let mut state = load_state();
    if !known_theme(&theme_id, &state.custom_themes) {
        return Err("未知主题".into());
    }
    state.theme_id = theme_id;
    save_state(&state)?;
    Ok(current_status())
}

#[tauri::command]
fn add_custom_theme(colors: ThemeColors) -> Result<AppStatus, String> {
    let _state_lock = state_lock()?;
    let colors = colors.normalize()?;
    let mut state = load_state();
    let id = next_custom_id(&state.custom_themes);
    state.custom_themes.push(NamedTheme {
        id: id.clone(),
        colors: colors.clone(),
    });
    state.custom = colors;
    state.theme_id = id;
    save_state(&state)?;
    Ok(current_status())
}

#[tauri::command]
fn save_custom_theme(colors: ThemeColors) -> Result<AppStatus, String> {
    let _state_lock = state_lock()?;
    let colors = colors.normalize()?;
    let mut state = load_state();
    if !is_custom_theme_id(&state.theme_id) {
        return Err("当前不是自定义主题".into());
    }
    let Some(theme) = state
        .custom_themes
        .iter_mut()
        .find(|theme| theme.id == state.theme_id)
    else {
        return Err("未找到该自定义主题".into());
    };
    theme.colors = colors.clone();
    state.custom = colors;
    save_state(&state)?;
    Ok(current_status())
}

#[tauri::command]
fn delete_custom_theme(theme_id: String) -> Result<AppStatus, String> {
    let _state_lock = state_lock()?;
    if !is_custom_theme_id(&theme_id) {
        return Err("只能删除自定义主题".into());
    }
    let mut state = load_state();
    let before = state.custom_themes.len();
    state.custom_themes.retain(|theme| theme.id != theme_id);
    if state.custom_themes.len() == before {
        return Err("未找到该自定义主题".into());
    }
    if state.theme_id == theme_id {
        state.theme_id = "dark".into();
    }
    save_state(&state)?;
    Ok(current_status())
}

#[tauri::command]
fn set_fonts(fonts: FontChoice) -> Result<AppStatus, String> {
    let _state_lock = state_lock()?;
    let mut state = load_state();
    state.fonts = fonts.normalize()?;
    save_state(&state)?;
    Ok(current_status())
}

#[tauri::command]
fn set_glass_opacity(opacity: u8) -> Result<AppStatus, String> {
    let _state_lock = state_lock()?;
    let mut state = load_state();
    state.glass_opacity = clamp_glass_opacity(opacity);
    save_state(&state)?;
    Ok(current_status())
}

#[tauri::command]
fn list_system_fonts() -> FontLists {
    list_installed()
}

#[tauri::command]
fn get_user_logo() -> Option<Vec<u8>> {
    load_user_logo()
}

#[tauri::command]
fn get_toolbar_preview(
    colors: Option<ThemeColors>,
    glass_opacity: Option<u8>,
) -> Result<ToolbarPreview, String> {
    let state = load_state();
    let src = official_status_wnd().ok_or_else(|| "未找到输入法工具栏资源".to_string())?;
    let palette = match colors {
        Some(c) => c.normalize()?,
        None => colors_for(&state),
    };
    let opacity = clamp_glass_opacity(glass_opacity.unwrap_or(state.glass_opacity));
    let glass = glass_enabled(opacity);
    let custom = load_user_logo();
    generate_toolbar_preview(
        &src,
        &palette,
        false,
        GLASS_OPACITY_DEFAULT,
        state.theme_id == "light" && !glass && palette == light_colors(),
        custom.as_deref(),
    )
}

#[tauri::command]
fn clear_logo() -> Result<AppStatus, String> {
    let _state_lock = state_lock()?;
    let path = workdir::user_logo_path();
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("清除头像失败：{e}"))?;
    }
    Ok(current_status())
}

const PUBLIC_REPO_URL: &str = "https://github.com/tori-RR/DoubaoIME_Darkmode";

fn open_folder(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开目录失败：{e}"))
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("打开链接失败：{e}"))
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("仅支持 Windows".into())
    }
}

#[tauri::command]
fn open_workdir() -> Result<(), String> {
    let dir = workdir::ensure_app_dir()?;
    open_folder(&dir)
}

#[tauri::command]
fn open_public_repo() -> Result<(), String> {
    if PUBLIC_REPO_URL.is_empty() {
        return Err("公开仓库尚未发布".into());
    }
    open_url(PUBLIC_REPO_URL)
}

const SETTINGS_LAUNCHER: &str = r"C:\Program Files\DoubaoIME\bootstrap\SettingsLauncher.exe";

fn open_exe(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("未找到输入法设置".into());
    }
    std::process::Command::new(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开设置失败：{e}"))
}

#[tauri::command]
fn open_ime_settings() -> Result<(), String> {
    open_exe(Path::new(SETTINGS_LAUNCHER))
}

#[tauri::command]
async fn ime_toolbar_visible() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(ime_rpc::toolbar_enabled)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn toggle_ime_toolbar() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(|| {
        if workdir::review_mode() {
            return Err("检查模式不改变输入法工具栏".into());
        }
        ime_rpc::toggle_toolbar()
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    if let Some(job) = elevate::take_job() {
        let code = match job.and_then(|job| skin::run_job(&job)) {
            Ok(()) => 0,
            Err(_) => 1,
        };
        std::process::exit(code);
    }
    migrate_legacy_state();
    let webview_dir = workdir::ensure_app_dir()
        .expect("工作目录不可写")
        .join("webview")
        .join(&safe_fs::hash(std::env::var("USERNAME").unwrap_or_default().as_bytes())[..16]);
    tauri::Builder::default()
        .setup(move |app| {
            let cfg = app
                .config()
                .app
                .windows
                .first()
                .expect("main window configuration");
            let mut window =
                tauri::WebviewWindowBuilder::from_config(app, cfg)?.data_directory(webview_dir);
            if workdir::review_mode() {
                window = window.title("DoubaoIME Darkmode · 检查模式");
            }
            window.build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            set_theme_id,
            add_custom_theme,
            save_custom_theme,
            delete_custom_theme,
            set_fonts,
            set_glass_opacity,
            list_system_fonts,
            get_user_logo,
            get_toolbar_preview,
            import_logo,
            clear_logo,
            install,
            uninstall,
            open_workdir,
            open_ime_settings,
            ime_toolbar_visible,
            toggle_ime_toolbar,
            open_public_repo,
            get_kaomoji_groups
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod upload_tests {
    use super::upload_bytes;
    use tauri::ipc::InvokeBody;

    #[test]
    fn accepts_raw_and_json_byte_payloads() {
        let expected = vec![0, 1, 127, 255];
        assert_eq!(
            upload_bytes(&InvokeBody::Raw(expected.clone())).unwrap(),
            expected
        );
        assert_eq!(
            upload_bytes(&InvokeBody::Json(serde_json::json!([0, 1, 127, 255]))).unwrap(),
            expected
        );
    }

    #[test]
    fn rejects_non_byte_json_payloads() {
        for value in [
            serde_json::json!({ "0": 1 }),
            serde_json::json!([256]),
            serde_json::json!(["1"]),
        ] {
            assert!(upload_bytes(&InvokeBody::Json(value)).is_err());
        }
    }
}
