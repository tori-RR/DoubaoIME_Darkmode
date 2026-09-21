use crate::{
    avatar,
    fonts::FontChoice,
    ime_process, safe_fs, theme,
    transaction::{Files, Hashes, Store},
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const IME_ROOT: &str = r"C:\Program Files\DoubaoIME";
pub const IME_VERSION: &str = "v0.9.0.0";
// Versions explicitly verified for this release, never an implicit <= range.
pub const VERIFIED_IME_VERSIONS: &[&str] = &[IME_VERSION];
pub const BACKUP_DIR_NAME: &str = crate::transaction::BACKUP;

pub fn official_hashes() -> Hashes {
    serde_json::from_str(include_str!("official-hashes.json")).expect("checked adapter manifest")
}
fn skin_for(version: &str) -> PathBuf {
    PathBuf::from(IME_ROOT)
        .join("versions")
        .join(version)
        .join(r"files\data\skin\default")
}
/// Read-only layout probe, deliberately independent of release coverage and
/// official hashes. A matching layout lets an unknown release continue to the
/// semantic patch-generation preflight; it does not mark that release verified.
pub fn inspect_structure(dir: &Path) -> Result<(), String> {
    read_managed_files(dir).map(|_| ())
}

fn read_managed_files(dir: &Path) -> Result<Files, String> {
    official_hashes()
        .keys()
        .map(|rel| {
            let bytes = safe_fs::read(&dir.join(rel), 4 * 1024 * 1024)
                .map_err(|err| format!("必需文件 {rel} 缺失或无法读取：{err}"))?;
            if bytes.is_empty() {
                return Err(format!("必需文件 {rel} 为空"));
            }
            Ok((rel.clone(), bytes))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct DetectedIme {
    pub version: String,
    pub skin: PathBuf,
}

#[derive(Deserialize)]
struct BootstrapManifest {
    bootstrap_version: String,
}

fn bootstrap_version(raw: &[u8]) -> Result<String, String> {
    let raw = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
    serde_json::from_slice::<BootstrapManifest>(raw)
        .map(|manifest| manifest.bootstrap_version)
        .map_err(|err| format!("输入法引导清单无效：{err}"))
}

/// Build the transactional store for the detected version. The pinned release
/// keeps its stronger official hash baseline. For an unverified version, the
/// first install snapshots the current structurally compatible files; later
/// runs reconstruct that baseline only from the retained backup.
pub fn store_for(target: &DetectedIme) -> Result<Store, String> {
    inspect_structure(&target.skin)?;
    let baseline = if target.version == IME_VERSION {
        official_hashes()
    } else {
        let backup = target.skin.join(BACKUP_DIR_NAME);
        let originals = if backup.is_dir() {
            read_managed_files(&backup)?
        } else {
            read_managed_files(&target.skin)?
        };
        Store::hashes(&originals)
    };
    Ok(Store::new(
        target.skin.clone(),
        target.version.clone(),
        baseline,
    ))
}

pub fn detect() -> Result<DetectedIme, String> {
    let root = Path::new(IME_ROOT);
    if !root.is_dir() {
        return Err("未安装豆包输入法".into());
    }
    let processes = ime_process::processes()?;
    let versions = root.join("versions");
    let mut active = Vec::new();
    for p in processes.iter().filter(|p| {
        p.path
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("ImeService.exe"))
    }) {
        let rel = p
            .path
            .strip_prefix(&versions)
            .map_err(|_| "发现其他安装目录的输入法，请先退出后重试")?;
        let parts: Vec<_> = rel.components().collect();
        if parts.len() != 2 {
            return Err("无法识别活动输入法路径".into());
        }
        active.push(parts[0].as_os_str().to_string_lossy().into_owned());
    }
    active.sort();
    active.dedup();
    let version = match active.len() {
        1 => active.remove(0),
        0 => {
            // Offline detection is conservative: exactly one version, matching bootstrap.
            let dirs: Vec<_> = fs::read_dir(&versions)
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|e| e.path().is_dir())
                .collect();
            let bootstrap = bootstrap_version(&safe_fs::read(
                &root.join("bootstrap/bootstrap_manifest.json"),
                65536,
            )?)?;
            if dirs.len() != 1 {
                return Err("多版本并存，需启动豆包输入法才能确认活动版本".into());
            }
            let name = dirs[0].file_name().to_string_lossy().into_owned();
            if Some(bootstrap.as_str()) != name.strip_prefix('v') {
                return Err("引导版本与安装目录不一致".into());
            }
            name
        }
        _ => return Err("多个输入法版本正在使用，暂不能修改".into()),
    };
    Ok(DetectedIme {
        skin: skin_for(&version),
        version,
    })
}

/// Recover using a journaled skin directory when live version detection cannot
/// run, e.g. IME stopped and several version folders remain. Never picks the
/// newest folder; only a single unfinished journal is accepted.
pub fn interrupted_ime() -> Option<DetectedIme> {
    let versions = Path::new(IME_ROOT).join("versions");
    let entries = fs::read_dir(&versions).ok()?;
    let mut found = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let version = entry.file_name().to_string_lossy().into_owned();
        let target = DetectedIme {
            skin: skin_for(&version),
            version,
        };
        let Ok(store) = store_for(&target) else {
            continue;
        };
        if store.recovery_needed().ok()? {
            found.push(target);
        }
    }
    if found.len() == 1 {
        found.pop()
    } else {
        None
    }
}

pub fn detect_for_status() -> Result<DetectedIme, String> {
    match detect() {
        Ok(target) => Ok(target),
        Err(err) => interrupted_ime().ok_or(err),
    }
}
pub fn originals_dir() -> Result<PathBuf, String> {
    let target = detect()?;
    store_for(&target)?.originals()?;
    let backup = target.skin.join(BACKUP_DIR_NAME);
    Ok(if backup.is_dir() { backup } else { target.skin })
}
#[cfg(test)]
pub fn read_original_text(originals: &Path, rel: &str) -> Result<String, String> {
    String::from_utf8(read_original_bytes(originals, rel)?).map_err(|e| e.to_string())
}
#[cfg(test)]
pub fn read_original_bytes(originals: &Path, rel: &str) -> Result<Vec<u8>, String> {
    if !official_hashes().contains_key(rel) {
        return Err("不受支持的皮肤文件".into());
    }
    safe_fs::read(&originals.join(rel), 4 * 1024 * 1024)
}
#[cfg(test)]
pub fn copy_original(originals: &Path, rel: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(dest, read_original_bytes(originals, rel)?).map_err(|e| e.to_string())
}
pub fn official_status_wnd() -> Option<PathBuf> {
    originals_dir().ok().map(|p| p.join("status_wnd"))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Apply,
    Restore,
    Recover,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub action: Action,
    pub version: String,
    pub expected: Hashes,
    pub colors: theme::ThemeColors,
    pub fonts: FontChoice,
    pub opacity: u8,
    pub light: bool,
    pub logo: Option<Vec<u8>>,
    #[serde(default)]
    pub taskbar: crate::taskbar_icon::Request,
    #[serde(default)]
    pub expected_taskbar: Option<crate::taskbar_icon::Snapshot>,
    pub label: String,
    pub id: String,
}
impl Job {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_ime_version(&self.version)
            || self.id.is_empty()
            || self.id.len() > 80
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err("不支持的安装请求".into());
        }
        self.colors.clone().normalize()?;
        self.fonts.clone().normalize()?;
        if !(20..=100).contains(&self.opacity) || self.label.chars().count() > 100 {
            return Err("主题参数无效".into());
        }
        if let Some(logo) = &self.logo {
            avatar::validate_stored(logo)?;
        }
        crate::taskbar_icon::validate(&self.taskbar)?;
        if matches!(self.taskbar, crate::taskbar_icon::Request::Keep)
            != self.expected_taskbar.is_none()
        {
            return Err("任务栏图标请求缺少快照或参数不一致".into());
        }
        if !matches!(self.action, Action::Apply)
            && matches!(self.taskbar, crate::taskbar_icon::Request::Custom { .. })
        {
            return Err("恢复操作不能写入自定义任务栏图标".into());
        }
        if self.expected.keys().ne(official_hashes().keys())
            || self
                .expected
                .values()
                .any(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("补丁文件清单不完整".into());
        }
        Ok(())
    }
    pub fn generate(&self, originals: &Files) -> Result<Files, String> {
        self.validate()?;
        theme::generate_patch(
            originals,
            &self.colors,
            &self.fonts,
            self.light,
            self.opacity,
            self.logo.as_deref(),
        )
    }
}

fn valid_ime_version(version: &str) -> bool {
    let Some(rest) = version.strip_prefix('v') else {
        return false;
    };
    !rest.is_empty()
        && version.len() <= 64
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && !rest.ends_with(['.', '-', '_'])
}

#[cfg(test)]
mod structure_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static ID: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dmdm-structure-{}-{}",
                std::process::id(),
                ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let fixture = Self(root);
            // Synthetic bytes only; structure inspection must not compare
            // official hashes or implicitly verify an unknown version.
            for rel in official_hashes().keys() {
                safe_fs::atomic_write(&fixture.skin().join(rel), b"synthetic asset").unwrap();
            }
            fixture
        }
        fn skin(&self) -> PathBuf {
            self.0.join("versions/v9.9.9.9/files/data/skin/default")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn matching_structure_is_read_only_and_independent_of_version_and_hashes() {
        let fixture = Fixture::new();
        let target = DetectedIme {
            version: "v9.9.9.9".into(),
            skin: fixture.skin(),
        };
        let store = store_for(&target).unwrap();
        let before = store.live_hashes().unwrap();
        assert_ne!(before, official_hashes());
        assert!(inspect_structure(&fixture.skin()).is_ok());
        assert_eq!(
            store.originals().unwrap(),
            read_managed_files(&fixture.skin()).unwrap()
        );
        assert_eq!(store.live_hashes().unwrap(), before);
        assert!(!VERIFIED_IME_VERSIONS.contains(&"v9.9.9.9"));
    }

    #[test]
    fn unverified_version_reuses_retained_baseline_after_apply() {
        let fixture = Fixture::new();
        let target = DetectedIme {
            version: "v9.9.9.9".into(),
            skin: fixture.skin(),
        };
        let original = read_managed_files(&target.skin).unwrap();
        let store = store_for(&target).unwrap();
        let expected = store.live_hashes().unwrap();
        let next: Files = original
            .keys()
            .map(|rel| (rel.clone(), format!("themed {rel}").into_bytes()))
            .collect();
        store.apply(&next, &expected, "test", "job").unwrap();

        let reopened = store_for(&target).unwrap();
        assert_eq!(reopened.originals().unwrap(), original);
        assert_eq!(reopened.live_hashes().unwrap(), Store::hashes(&next));
        assert_eq!(
            reopened.manifest().unwrap().unwrap().applied,
            Store::hashes(&next)
        );
    }

    #[test]
    fn job_version_accepts_safe_future_names_and_rejects_paths() {
        let job = |version: &str| Job {
            action: Action::Apply,
            version: version.into(),
            expected: official_hashes(),
            colors: theme::dark_colors(),
            fonts: FontChoice::default(),
            opacity: 100,
            light: false,
            logo: None,
            taskbar: crate::taskbar_icon::Request::Keep,
            expected_taskbar: None,
            label: "test".into(),
            id: "job".into(),
        };
        assert!(job("v0.10.0.0").validate().is_ok());
        assert!(job("v1.0.0-beta1").validate().is_ok());
        for version in ["0.10.0.0", "v", "v../escape", "v0\\escape", "v0:escape"] {
            assert!(job(version).validate().is_err());
        }
    }

    #[test]
    fn bootstrap_manifest_accepts_utf8_bom() {
        let plain = br#"{"bootstrap_version":"0.9.0.0"}"#;
        let mut with_bom = vec![0xef, 0xbb, 0xbf];
        with_bom.extend_from_slice(plain);
        assert_eq!(bootstrap_version(plain).unwrap(), "0.9.0.0");
        assert_eq!(bootstrap_version(&with_bom).unwrap(), "0.9.0.0");
        assert!(bootstrap_version(b"").unwrap_err().contains("引导清单无效"));
    }

    #[test]
    fn missing_required_file_reports_its_path() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.skin().join("window.xml")).unwrap();
        assert!(inspect_structure(&fixture.skin())
            .unwrap_err()
            .contains("window.xml"));
    }

    #[test]
    fn empty_required_file_is_not_a_matching_structure() {
        let fixture = Fixture::new();
        safe_fs::atomic_write(&fixture.skin().join("page_open.svg"), b"").unwrap();
        assert!(inspect_structure(&fixture.skin())
            .unwrap_err()
            .contains("page_open.svg 为空"));
    }

    #[test]
    fn directory_cannot_replace_a_required_file() {
        let fixture = Fixture::new();
        let path = fixture.skin().join("status_wnd/logo.png");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(inspect_structure(&fixture.skin())
            .unwrap_err()
            .contains("status_wnd/logo.png"));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[test]
    #[ignore = "read-only integration check requires this machine's verified IME installation"]
    fn installed_adapter_read_only() {
        let installed = detect().unwrap();
        assert_eq!(installed.version, IME_VERSION);
        assert!(inspect_structure(&installed.skin).is_ok());
        let store = store_for(&installed).unwrap();
        let original = store.originals().unwrap();
        assert_eq!(original.len(), 29);
        for (light, colors, opacity) in [
            (false, theme::dark_colors(), 100),
            (false, theme::mix_colors(), 70),
            (true, theme::light_colors(), 100),
        ] {
            let files = theme::generate_patch(
                &original,
                &colors,
                &FontChoice::default(),
                light,
                opacity,
                None,
            )
            .unwrap();
            assert_eq!(
                files.keys().collect::<Vec<_>>(),
                original.keys().collect::<Vec<_>>()
            );
            assert_eq!(
                files["status_wnd/logo.png"],
                original["status_wnd/logo.png"]
            );
            if light {
                for (name, bytes) in &original {
                    if name != "window.xml" {
                        assert_eq!(&files[name], bytes);
                    }
                }
            }
        }
        println!(
            "Verified adapter: {} / {} files; toolbar enabled: {}",
            installed.version,
            original.len(),
            crate::ime_rpc::toolbar_enabled().unwrap()
        );
    }
}
fn target_for_job(job: &Job) -> Result<DetectedIme, String> {
    match detect() {
        Ok(target) => {
            if target.version != job.version {
                return Err("输入法版本发生变化，请重新打开助手".into());
            }
            Ok(target)
        }
        Err(err) if matches!(job.action, Action::Recover) => {
            let target = DetectedIme {
                skin: skin_for(&job.version),
                version: job.version.clone(),
            };
            let store = store_for(&target)?;
            if store.recovery_needed()? || crate::taskbar_icon::recovery_needed()? {
                Ok(target)
            } else {
                Err(err)
            }
        }
        Err(err) => Err(err),
    }
}

fn preflight_target(job: &Job, target: &DetectedIme) -> Result<(), String> {
    let store = store_for(target)?;
    if matches!(job.action, Action::Recover) {
        if !store.recovery_needed()? && !crate::taskbar_icon::recovery_needed()? {
            return Err("没有待恢复事务".into());
        }
    } else {
        if store.recovery_needed()? || crate::taskbar_icon::recovery_needed()? {
            return Err("存在中断事务，请先恢复".into());
        }
        let originals = store.originals()?;
        if let Some(m) = store.manifest()? {
            if m.applied != job.expected {
                return Err("皮肤已被其他程序修改，拒绝覆盖".into());
            }
        }
        if matches!(job.action, Action::Apply) {
            job.generate(&originals)?;
        }
        if store.live_hashes()? != job.expected {
            return Err("预检后皮肤发生变化".into());
        }
        if let Some(expected) = &job.expected_taskbar {
            if crate::taskbar_icon::read_registry()? != *expected {
                return Err("任务栏图标在预检后发生变化，请重试".into());
            }
        }
    }
    Ok(())
}

pub fn preflight(job: &Job) -> Result<(), String> {
    job.validate()?;
    let target = target_for_job(job)?;
    preflight_target(job, &target)
}

pub fn job_skin(job: &Job) -> Result<PathBuf, String> {
    job.validate()?;
    Ok(skin_for(&job.version))
}

#[derive(Serialize, Deserialize)]
pub struct JobResult {
    pub id: String,
    pub error: Option<String>,
    #[serde(default)]
    pub taskbar_changed: bool,
}
pub fn run_job(job: &Job) -> Result<(), String> {
    job.validate()?;
    let root = job_skin(job)?;
    let _guards = safe_fs::guard_path(&root.join("status_wnd"))?;
    let _lock = safe_fs::exclusive_lock(&root.join(".dmdm-install.lock"))?;
    let mut taskbar_changed = false;
    let result = (|| {
        let target = target_for_job(job)?;
        preflight_target(job, &target)?;
        let store = store_for(&target)?;
        let next = match job.action {
            Action::Apply => Some(job.generate(&store.originals()?)?),
            Action::Restore => Some(store.originals()?),
            Action::Recover => None,
        };
        ime_process::stop(Path::new(IME_ROOT), &job.version)?;
        match next {
            Some(files) => {
                // The icon journal is durable before the first skin write.
                // A failed skin transaction rolls the icon reference back too;
                // a killed helper leaves both recovery paths discoverable.
                let undo = crate::taskbar_icon::apply(
                    &job.taskbar,
                    job.expected_taskbar.as_ref(),
                    &job.id,
                )?;
                if let Err(err) = store.apply(
                    &files,
                    &job.expected,
                    if matches!(job.action, Action::Restore) {
                        "官方浅色"
                    } else {
                        &job.label
                    },
                    &job.id,
                ) {
                    return match undo.rollback() {
                        Ok(()) => Err(err),
                        Err(rollback) => Err(format!("{err}；任务栏图标回滚未完成：{rollback}")),
                    };
                }
                taskbar_changed = undo.changed;
                undo.finish()
            }
            None => {
                if store.recovery_needed()? {
                    store.recover()?;
                }
                if crate::taskbar_icon::recovery_needed()? {
                    // If skin committed just before the helper stopped, finish
                    // its matching icon commit instead of undoing half the job.
                    let committed = store.manifest()?.filter(|manifest| {
                        store.live_hashes().ok().as_ref() == Some(&manifest.applied)
                    });
                    taskbar_changed = crate::taskbar_icon::recover(
                        committed.as_ref().map(|manifest| manifest.job.as_str()),
                    )?;
                }
                Ok(())
            }
        }
    })();
    let receipt = JobResult {
        id: job.id.clone(),
        error: result.clone().err(),
        taskbar_changed,
    };
    safe_fs::atomic_write(
        &root.join("dmdm_last_result.json"),
        &serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
    )?;
    result
}
