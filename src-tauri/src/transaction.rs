//! Durable, recoverable multi-file commits. Permanent originals are never deleted.
use crate::safe_fs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub type Files = BTreeMap<String, Vec<u8>>;
pub type Hashes = BTreeMap<String, String>;
pub const BACKUP: &str = "dmdm_backup";
const MANIFEST: &str = "dmdm_manifest.json";
const JOURNAL: &str = "dmdm_transaction/journal.json";
const LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub target: String,
    pub version: String,
    pub originals: Hashes,
    pub applied: Hashes,
    pub label: String,
    pub job: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    target: String,
    phase: String,
    before: Hashes,
    after: Hashes,
    old_manifest: Option<Vec<u8>>,
    new_manifest: Manifest,
}

pub struct Store {
    root: PathBuf,
    version: String,
    official: Hashes,
}
impl Store {
    pub fn new(root: PathBuf, version: String, official: Hashes) -> Self {
        Self {
            root,
            version,
            official,
        }
    }
    fn identity(&self) -> String {
        self.root.to_string_lossy().to_ascii_lowercase()
    }
    pub fn hashes(files: &Files) -> Hashes {
        files
            .iter()
            .map(|(k, v)| (k.clone(), safe_fs::hash(v)))
            .collect()
    }
    fn read_set(&self, root: &Path) -> Result<Files, String> {
        self.official
            .keys()
            .map(|rel| Ok((rel.clone(), safe_fs::read(&root.join(rel), LIMIT)?)))
            .collect()
    }
    pub fn live_hashes(&self) -> Result<Hashes, String> {
        Ok(Self::hashes(&self.read_set(&self.root)?))
    }
    fn check_set(&self, hashes: &Hashes) -> Result<(), String> {
        if hashes.keys().ne(self.official.keys())
            || hashes
                .values()
                .any(|h| h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err("记录中的文件清单不完整或不受支持".into());
        }
        Ok(())
    }
    pub fn manifest(&self) -> Result<Option<Manifest>, String> {
        let path = self.root.join(MANIFEST);
        if !path.try_exists().map_err(|e| e.to_string())? {
            return Ok(None);
        }
        let m: Manifest = serde_json::from_slice(&safe_fs::read(&path, LIMIT)?)
            .map_err(|e| format!("安装记录损坏：{e}"))?;
        if m.schema != 1
            || m.target != self.identity()
            || m.version != self.version
            || m.originals != self.official
        {
            return Err("安装记录不适用于当前输入法版本或目录".into());
        }
        self.check_set(&m.applied)?;
        Ok(Some(m))
    }
    pub fn originals(&self) -> Result<Files, String> {
        let backup = self.root.join(BACKUP);
        let root = if backup.try_exists().map_err(|e| e.to_string())? {
            &backup
        } else {
            &self.root
        };
        let files = self
            .read_set(root)
            .map_err(|e| format!("原件/备份不完整，已停止操作：{e}"))?;
        if Self::hashes(&files) != self.official {
            return Err(
                "原件/备份校验失败：需要对应版本的完整官方原件，不能从已修改的皮肤补齐".into(),
            );
        }
        Ok(files)
    }
    pub fn has_backup(&self) -> bool {
        self.root.join(BACKUP).is_dir()
    }
    pub fn recovery_needed(&self) -> Result<bool, String> {
        let p = self.root.join(JOURNAL);
        if !p.try_exists().map_err(|e| e.to_string())? {
            return Ok(false);
        }
        let j: Journal = serde_json::from_slice(&safe_fs::read(&p, LIMIT)?)
            .map_err(|e| format!("事务记录损坏：{e}"))?;
        self.validate_journal(&j)?;
        Ok(!matches!(j.phase.as_str(), "committed" | "rolled_back"))
    }
    fn validate_journal(&self, j: &Journal) -> Result<(), String> {
        if j.schema != 1
            || j.target != self.identity()
            || !matches!(
                j.phase.as_str(),
                "prepared" | "writing" | "committed" | "rolled_back"
            )
        {
            return Err("事务记录不适用于当前目录".into());
        }
        self.check_set(&j.before)?;
        self.check_set(&j.after)?;
        if let Some(raw) = &j.old_manifest {
            let m: Manifest = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
            if m.schema != 1
                || m.target != self.identity()
                || m.version != self.version
                || m.originals != self.official
            {
                return Err("事务中的原安装记录无效".into());
            }
            self.check_set(&m.applied)?;
        }
        Ok(())
    }
    fn save_journal(&self, j: &Journal) -> Result<(), String> {
        safe_fs::atomic_write(
            &self.root.join(JOURNAL),
            &serde_json::to_vec_pretty(j).map_err(|e| e.to_string())?,
        )
    }
    fn ensure_backup(&self, originals: &Files) -> Result<(), String> {
        if self.has_backup() {
            self.originals()?;
            return Ok(());
        }
        let staging = self.root.join("dmdm_backup_pending");
        if staging.try_exists().map_err(|e| e.to_string())? {
            return Err(
                "发现未完成的首次备份 dmdm_backup_pending；原皮肤未覆盖，请先检查并保留该目录"
                    .into(),
            );
        }
        fs::create_dir(&staging).map_err(|e| e.to_string())?;
        for (rel, bytes) in originals {
            safe_fs::atomic_write(&staging.join(rel), bytes)?;
        }
        if Self::hashes(&self.read_set(&staging)?) != self.official {
            return Err("暂存备份校验失败".into());
        }
        fs::rename(&staging, self.root.join(BACKUP))
            .map_err(|e| format!("提交原件备份失败：{e}"))?;
        self.originals()?;
        Ok(())
    }
    pub fn recover(&self) -> Result<(), String> {
        let mut j: Journal =
            serde_json::from_slice(&safe_fs::read(&self.root.join(JOURNAL), LIMIT)?)
                .map_err(|e| e.to_string())?;
        self.validate_journal(&j)?;
        if matches!(j.phase.as_str(), "committed" | "rolled_back") {
            return Ok(());
        }
        // Validate every recovery byte before the first write.
        let before = self.read_set(&self.root.join("dmdm_transaction/before"))?;
        if Self::hashes(&before) != j.before {
            return Err("事务快照损坏，请保留备份并手动恢复".into());
        }
        let live = self.live_hashes()?;
        for (rel, h) in &live {
            if Some(h) != j.before.get(rel) && Some(h) != j.after.get(rel) {
                return Err(format!("{rel} 在中断后被其他程序修改；保留快照，拒绝覆盖"));
            }
        }
        self.rollback(&mut j, &before)
    }
    fn rollback(&self, j: &mut Journal, before: &Files) -> Result<(), String> {
        for (rel, bytes) in before {
            // Do not touch unchanged files (for example a locked, unmodified logo).
            if safe_fs::hash(&safe_fs::read(&self.root.join(rel), LIMIT)?) != j.before[rel] {
                safe_fs::atomic_write(&self.root.join(rel), bytes)?;
            }
        }
        if self.live_hashes()? != j.before {
            return Err("回滚校验失败，已保留事务快照".into());
        }
        let manifest_path = self.root.join(MANIFEST);
        match &j.old_manifest {
            Some(raw) => safe_fs::atomic_write(&manifest_path, raw)?,
            None if manifest_path.exists() => {
                fs::remove_file(manifest_path).map_err(|e| e.to_string())?
            }
            None => (),
        }
        j.phase = "rolled_back".into();
        self.save_journal(j)
    }
    pub fn apply(
        &self,
        files: &Files,
        expected: &Hashes,
        label: &str,
        job: &str,
    ) -> Result<(), String> {
        self.commit_with(files, expected, label, job, |_| Ok(()))
    }
    fn commit_with(
        &self,
        files: &Files,
        expected: &Hashes,
        label: &str,
        job: &str,
        mut checkpoint: impl FnMut(usize) -> Result<(), String>,
    ) -> Result<(), String> {
        self.check_set(&Self::hashes(files))?;
        if self.recovery_needed()? {
            return Err("存在中断事务，请先恢复".into());
        }
        let before = self.read_set(&self.root)?;
        if &Self::hashes(&before) != expected {
            return Err("预检后皮肤发生变化，请重新确认".into());
        }
        let old_manifest = self.manifest()?;
        if let Some(m) = &old_manifest {
            if m.applied != *expected {
                return Err("皮肤被其他程序修改，拒绝覆盖".into());
            }
        }
        let originals = self.originals()?;
        self.ensure_backup(&originals)?;
        // Replace only our fixed snapshot files. The previous completed journal
        // remains valid until all new snapshots are flushed; no recursive delete.
        let txn = self.root.join("dmdm_transaction");
        for (rel, bytes) in &before {
            safe_fs::atomic_write(&txn.join("before").join(rel), bytes)?;
        }
        if Self::hashes(&self.read_set(&txn.join("before"))?) != *expected {
            return Err("操作前快照校验失败".into());
        }
        let next = Manifest {
            schema: 1,
            target: self.identity(),
            version: self.version.clone(),
            originals: self.official.clone(),
            applied: Self::hashes(files),
            label: label.into(),
            job: job.into(),
        };
        let mut j = Journal {
            schema: 1,
            target: self.identity(),
            phase: "prepared".into(),
            before: expected.clone(),
            after: next.applied.clone(),
            old_manifest: old_manifest.map(|m| serde_json::to_vec(&m).unwrap()),
            new_manifest: next,
        };
        self.save_journal(&j)?;
        j.phase = "writing".into();
        self.save_journal(&j)?;
        let result = (|| {
            for (i, (rel, bytes)) in files.iter().enumerate() {
                checkpoint(i)?;
                if j.before[rel] != j.after[rel] {
                    safe_fs::atomic_write(&self.root.join(rel), bytes)?;
                }
                if safe_fs::hash(&safe_fs::read(&self.root.join(rel), LIMIT)?) != j.after[rel] {
                    return Err(format!("{rel} 写入后校验失败"));
                }
            }
            checkpoint(files.len())?;
            safe_fs::atomic_write(
                &self.root.join(MANIFEST),
                &serde_json::to_vec_pretty(&j.new_manifest).map_err(|e| e.to_string())?,
            )?;
            j.phase = "committed".into();
            self.save_journal(&j)
        })();
        if let Err(err) = result {
            return match self.rollback(&mut j, &before) {
                Ok(()) => Err(format!("{err}；已恢复操作前皮肤")),
                Err(rollback) => Err(format!(
                    "{err}；回滚未完成：{rollback}，请保留 dmdm_transaction"
                )),
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn fixture() -> (PathBuf, Store, Files, Files) {
        static ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dmdm-txn-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let original: Files = [
            ("a.svg".into(), b"original a".to_vec()),
            ("status_wnd/logo.png".into(), b"original logo".to_vec()),
        ]
        .into();
        for (k, v) in &original {
            safe_fs::atomic_write(&root.join(k), v).unwrap();
        }
        let next: Files = original
            .keys()
            .map(|k| (k.clone(), format!("themed {k}").into_bytes()))
            .collect();
        let store = Store::new(root.clone(), "test".into(), Store::hashes(&original));
        (root, store, original, next)
    }
    #[test]
    fn every_commit_failure_rolls_back_and_keeps_originals() {
        for fail in 0..=2 {
            let (root, store, original, next) = fixture();
            assert!(store
                .commit_with(&next, &Store::hashes(&original), "new", "job", |i| {
                    if i == fail {
                        Err("injected".into())
                    } else {
                        Ok(())
                    }
                })
                .is_err());
            assert_eq!(store.live_hashes().unwrap(), Store::hashes(&original));
            assert_eq!(store.originals().unwrap(), original);
            assert!(!store.recovery_needed().unwrap());
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn second_theme_failure_restores_previous_theme_not_official() {
        let (root, store, original, next) = fixture();
        store
            .apply(&next, &Store::hashes(&original), "first", "a")
            .unwrap();
        assert!(store
            .commit_with(&original, &Store::hashes(&next), "second", "b", |i| {
                if i == 1 {
                    Err("fail".into())
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert_eq!(store.live_hashes().unwrap(), Store::hashes(&next));
        assert_eq!(store.manifest().unwrap().unwrap().label, "first");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn partial_backup_and_changed_live_fail_closed() {
        let (root, store, original, next) = fixture();
        fs::create_dir(root.join(BACKUP)).unwrap();
        assert!(store
            .apply(&next, &Store::hashes(&original), "new", "j")
            .is_err());
        assert_eq!(store.live_hashes().unwrap(), Store::hashes(&original));
        fs::remove_dir_all(root).unwrap();
        let (root, store, original, next) = fixture();
        safe_fs::atomic_write(&root.join("a.svg"), b"third party").unwrap();
        assert!(store
            .apply(&next, &Store::hashes(&original), "new", "j")
            .is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn interrupted_commit_recovers_from_verified_snapshot() {
        let (root, store, original, next) = fixture();
        store
            .apply(&next, &Store::hashes(&original), "new", "j")
            .unwrap();
        let mut journal: Journal =
            serde_json::from_slice(&safe_fs::read(&root.join(JOURNAL), LIMIT).unwrap()).unwrap();
        journal.phase = "writing".into();
        store.save_journal(&journal).unwrap();
        assert!(store.recovery_needed().unwrap());
        store.recover().unwrap();
        assert_eq!(store.live_hashes().unwrap(), Store::hashes(&original));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recovery_refuses_external_edits_and_corrupted_snapshot() {
        for corrupt_snapshot in [false, true] {
            let (root, store, original, next) = fixture();
            store
                .apply(&next, &Store::hashes(&original), "new", "j")
                .unwrap();
            let mut journal: Journal =
                serde_json::from_slice(&safe_fs::read(&root.join(JOURNAL), LIMIT).unwrap())
                    .unwrap();
            journal.phase = "writing".into();
            store.save_journal(&journal).unwrap();
            let target = if corrupt_snapshot {
                root.join("dmdm_transaction/before/a.svg")
            } else {
                root.join("a.svg")
            };
            safe_fs::atomic_write(&target, b"unrelated edit").unwrap();
            let live = store.live_hashes().unwrap();
            assert!(store.recover().is_err());
            assert_eq!(store.live_hashes().unwrap(), live);
            assert!(store.recovery_needed().unwrap());
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn locked_destination_preserves_previous_theme() {
        use std::os::windows::fs::OpenOptionsExt;
        let (root, store, original, next) = fixture();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(root.join("status_wnd/logo.png"))
            .unwrap();
        assert!(store
            .apply(&next, &Store::hashes(&original), "new", "j")
            .is_err());
        assert_eq!(store.live_hashes().unwrap(), Store::hashes(&original));
        drop(lock);
        assert_eq!(store.originals().unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }
}
