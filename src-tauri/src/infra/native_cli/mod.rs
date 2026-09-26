//! Shared node ownership boundary for native CRUD and generated gateway entries.
mod document;
mod files;
pub(crate) mod omp_settings;
pub(crate) mod profiles;
pub(crate) mod targets;
#[cfg(test)]
mod tests;

use crate::domain::native_cli::{
    validate_native_key, validate_provider, NativeFormat, NativeTarget,
};
use crate::shared::error::{AppError, AppResult};
pub(crate) use document::node_digest;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, Weak};

type TargetLocks = Mutex<HashMap<String, Weak<Mutex<()>>>>;
static LOCKS: OnceLock<TargetLocks> = OnceLock::new();

/// All native writers, including gateway generation, must enter this boundary.
pub(crate) fn with_target_lock<T>(
    target: &NativeTarget,
    operation: impl FnOnce(&NativeTargetSession<'_>) -> AppResult<T>,
) -> AppResult<T> {
    let key = targets::path_identity(Path::new(&target.agent_dir));
    let lock = {
        let mut locks = LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| {
                AppError::new("NATIVE_LOCK_FAILED", "Native target lock is unavailable")
            })?;
        locks.retain(|_, v| v.strong_count() > 0);
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(Mutex::new(()));
            locks.insert(key, Arc::downgrade(&lock));
            lock
        }
    };
    let _guard = lock
        .lock()
        .map_err(|_| AppError::new("NATIVE_LOCK_FAILED", "Native target lock is unavailable"))?;
    operation(&NativeTargetSession { target })
}

pub(crate) struct NativeDocumentSnapshot {
    pub revision: String,
    pub exists: bool,
    root: Value,
    bytes: Option<Vec<u8>>,
}
impl NativeDocumentSnapshot {
    pub fn providers(&self) -> &Map<String, Value> {
        static EMPTY: OnceLock<Map<String, Value>> = OnceLock::new();
        self.root
            .get("providers")
            .and_then(Value::as_object)
            .unwrap_or_else(|| EMPTY.get_or_init(Map::new))
    }
}

pub(crate) struct NativeNodePatch {
    pub native_key: String,
    pub expected_digest: Option<String>,
    pub replacement: Option<Value>,
}

pub(crate) struct NativePatchReceipt {
    pub after_revision: String,
    pub changed: bool,
    pub backup_path: Option<String>,
    target_id: String,
    changes: Vec<NativeNodePatch>,
}

pub(crate) struct NativeTargetSession<'a> {
    target: &'a NativeTarget,
}
impl NativeTargetSession<'_> {
    fn check_target(&self) -> AppResult<()> {
        let directory = targets::canonical_directory(Path::new(&self.target.agent_dir))?;
        if targets::path_identity(&directory)
            != targets::path_identity(Path::new(&self.target.agent_dir))
        {
            return Err(AppError::new(
                "NATIVE_TARGET_CHANGED",
                "Agent directory changed; reload the target",
            ));
        }
        let (selected, format, _) = targets::selected_model_file(self.target.client, &directory)?;
        if targets::path_identity(&selected)
            != targets::path_identity(Path::new(&self.target.models_path))
            || format != self.target.format
        {
            return Err(AppError::new(
                "NATIVE_TARGET_CHANGED",
                "Native model file priority changed; reload the target",
            ));
        }
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> AppResult<NativeDocumentSnapshot> {
        self.check_target()?;
        let bytes = crate::shared::fs::read_optional_file_with_max_len(
            Path::new(&self.target.models_path),
            document::MAX_DOCUMENT_BYTES,
        )
        .map_err(|_| {
            AppError::new(
                "NATIVE_READ_FAILED",
                "Native model file cannot be read safely; editing is disabled",
            )
        })?;
        let revision = bytes
            .as_ref()
            .map(|bytes| document::digest_bytes(bytes))
            .unwrap_or_else(|| "missing".into());
        let root = match &bytes {
            Some(bytes) => document::parse(self.target.client, self.target.format, bytes)?,
            None => serde_json::json!({"providers":{}}),
        };
        Ok(NativeDocumentSnapshot {
            revision,
            exists: bytes.is_some(),
            root,
            bytes,
        })
    }

    pub(crate) fn patch(
        &self,
        expected_revision: &str,
        patches: &[NativeNodePatch],
    ) -> AppResult<NativePatchReceipt> {
        self.patch_with_hook(expected_revision, patches, || Ok(()))
    }

    fn patch_with_hook(
        &self,
        expected_revision: &str,
        patches: &[NativeNodePatch],
        before_finalize: impl FnOnce() -> AppResult<()>,
    ) -> AppResult<NativePatchReceipt> {
        if !self.target.writable || self.target.format == NativeFormat::LegacyJson {
            return Err(AppError::new(
                "NATIVE_TARGET_READ_ONLY",
                "This native target is read-only",
            ));
        }
        let snapshot = self.snapshot()?;
        if snapshot.revision != expected_revision {
            return Err(AppError::new(
                "NATIVE_REVISION_CONFLICT",
                "Native document changed; reload before editing",
            ));
        }
        if patches.len() > 4096 {
            return Err(AppError::new(
                "NATIVE_INVALID_PATCH",
                "Too many node changes",
            ));
        }
        let mut keys = HashSet::new();
        for patch in patches {
            validate_native_key(&patch.native_key)?;
            if !keys.insert(&patch.native_key) {
                return Err(AppError::new(
                    "NATIVE_INVALID_PATCH",
                    "Duplicate node patch key",
                ));
            }
            let actual = snapshot.providers().get(&patch.native_key).map(node_digest);
            if actual != patch.expected_digest {
                return Err(AppError::new(
                    "NATIVE_NODE_CONFLICT",
                    "Native provider changed or its name is already in use",
                ));
            }
            if let Some(node) = &patch.replacement {
                validate_provider(self.target.client, node)?;
            }
        }
        let mut root = snapshot.root.clone();
        let map = root
            .as_object_mut()
            .expect("validated root")
            .entry("providers")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .expect("validated providers");
        let mut reverse = Vec::new();
        for patch in patches {
            let before = map.get(&patch.native_key).cloned();
            if before == patch.replacement {
                continue;
            }
            reverse.push(NativeNodePatch {
                native_key: patch.native_key.clone(),
                expected_digest: patch.replacement.as_ref().map(node_digest),
                replacement: before,
            });
            match &patch.replacement {
                Some(value) => {
                    map.insert(patch.native_key.clone(), value.clone());
                }
                None => {
                    map.remove(&patch.native_key);
                }
            }
        }
        let mut receipt = NativePatchReceipt {
            after_revision: snapshot.revision.clone(),
            changed: !reverse.is_empty(),
            backup_path: None,
            target_id: self.target.target_id.clone(),
            changes: reverse,
        };
        if !receipt.changed {
            return Ok(receipt);
        }
        let bytes = document::serialize(self.target.client, self.target.format, &root)?;
        let path = Path::new(&self.target.models_path);
        if let Some(original) = &snapshot.bytes {
            receipt.backup_path = Some(
                files::backup(path, original)?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        files::atomic_write(path, &bytes, snapshot.exists, || {
            before_finalize()?;
            if self.snapshot()?.revision != snapshot.revision {
                return Err(AppError::new(
                    "NATIVE_REVISION_CONFLICT",
                    "Native document changed while preparing the write",
                ));
            }
            Ok(())
        })?;
        receipt.after_revision = document::digest_bytes(&bytes);
        if self.snapshot()?.revision != receipt.after_revision {
            return Err(AppError::new(
                "NATIVE_POST_WRITE_CONFLICT",
                "Native file changed after replacement; inspect current state and retained backup",
            ));
        }
        Ok(receipt)
    }

    pub(crate) fn compensate(&self, receipt: &NativePatchReceipt) -> AppResult<()> {
        if receipt.target_id != self.target.target_id {
            return Err(AppError::new(
                "NATIVE_COMPENSATION_CONFLICT",
                "Compensation target does not match",
            ));
        }
        if !receipt.changed {
            return Ok(());
        }
        let current = self.snapshot()?;
        for change in &receipt.changes {
            if current.providers().get(&change.native_key).map(node_digest)
                != change.expected_digest
            {
                return Err(AppError::new("NATIVE_COMPENSATION_CONFLICT","Native provider was externally edited; preserve current file and inspect backup"));
            }
        }
        self.patch(&current.revision, &receipt.changes).map(|_| ())
    }
}
