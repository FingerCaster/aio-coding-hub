//! Usage: Bounded backend-owned state for provider share import previews.

use crate::db;
use crate::providers::{
    parse_provider_share, preview_provider_share, ProviderAuthMode, ProviderShareCredentialStatus,
    ProviderShareEnvelopeV2, ProviderShareExtensionPreview, ProviderSharePreviewDraft,
    PROVIDER_SHARE_MAX_BYTES,
};
use crate::shared::error::{AppError, AppResult};
use crate::shared::mutex_ext::MutexExt;
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PREVIEW_TTL: Duration = Duration::from_secs(10 * 60);
const PREVIEW_MAX_ENTRIES: usize = 8;
const PREVIEW_MAX_SENSITIVE_BYTES: usize = 32 * 1024 * 1024;
const PREVIEW_TOKEN_BYTES: usize = 32;
const PREVIEW_TOKEN_HEX_LEN: usize = PREVIEW_TOKEN_BYTES * 2;

#[derive(Clone, serde::Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderShareImportPreview {
    pub(crate) preview_token: String,
    pub(crate) cli_key: String,
    pub(crate) source_name: String,
    pub(crate) final_name: String,
    pub(crate) source_enabled: bool,
    pub(crate) import_enabled: bool,
    pub(crate) auth_mode: ProviderAuthMode,
    pub(crate) credential_status: ProviderShareCredentialStatus,
    pub(crate) extension_count: u32,
    pub(crate) extensions: Vec<ProviderShareExtensionPreview>,
    pub(crate) can_import: bool,
}

struct PreviewSource {
    digest: [u8; 32],
    file_path: Option<PathBuf>,
}

struct PreviewEntry {
    token: String,
    created_at: Instant,
    sensitive_bytes: usize,
    envelope: ProviderShareEnvelopeV2,
    expected_final_name: String,
    expected_extensions: Vec<ProviderShareExtensionPreview>,
    source: PreviewSource,
}

#[derive(Default)]
struct PreviewCache {
    entries: VecDeque<PreviewEntry>,
    sensitive_bytes: usize,
    cleanup_worker_running: bool,
}

#[derive(Clone)]
pub(crate) struct ProviderShareService {
    cache: Arc<Mutex<PreviewCache>>,
    ttl: Duration,
    max_entries: usize,
    max_sensitive_bytes: usize,
}

impl Default for ProviderShareService {
    fn default() -> Self {
        Self::with_limits(
            PREVIEW_TTL,
            PREVIEW_MAX_ENTRIES,
            PREVIEW_MAX_SENSITIVE_BYTES,
        )
    }
}

impl ProviderShareService {
    fn with_limits(ttl: Duration, max_entries: usize, max_sensitive_bytes: usize) -> Self {
        Self {
            cache: Arc::new(Mutex::new(PreviewCache::default())),
            ttl,
            max_entries,
            max_sensitive_bytes,
        }
    }

    pub(crate) fn preview_content(
        &self,
        db: &db::Db,
        bytes: &[u8],
    ) -> AppResult<ProviderShareImportPreview> {
        self.preview_bytes(db, bytes, None, Instant::now())
    }

    pub(crate) fn preview_file(
        &self,
        db: &db::Db,
        path: PathBuf,
    ) -> AppResult<ProviderShareImportPreview> {
        let bytes = read_share_file(&path)?;
        self.preview_bytes(db, &bytes, Some(path), Instant::now())
    }

    fn preview_bytes(
        &self,
        db: &db::Db,
        bytes: &[u8],
        file_path: Option<PathBuf>,
        now: Instant,
    ) -> AppResult<ProviderShareImportPreview> {
        let envelope = parse_provider_share(bytes)?;
        let draft = preview_provider_share(db, &envelope)?;
        let source = PreviewSource {
            digest: digest_bytes(bytes),
            file_path,
        };
        let token = self.insert_at(
            envelope,
            draft.final_name.clone(),
            draft.extensions.clone(),
            source,
            bytes.len(),
            now,
        )?;
        Ok(preview_from_draft(token, draft))
    }

    pub(crate) fn take_preview(&self, token: &str) -> AppResult<PendingProviderShareImport> {
        self.take_at(token, Instant::now())
    }

    fn take_at(&self, token: &str, now: Instant) -> AppResult<PendingProviderShareImport> {
        validate_preview_token(token)?;
        let mut cache = self.cache.lock_or_recover();
        purge_expired(&mut cache, now, self.ttl);
        let position = cache.entries.iter().position(|entry| entry.token == token);
        let Some(position) = position else {
            return Err(preview_expired());
        };
        let entry = cache.entries.remove(position).ok_or_else(preview_expired)?;
        cache.sensitive_bytes = cache.sensitive_bytes.saturating_sub(entry.sensitive_bytes);
        Ok(PendingProviderShareImport { entry })
    }

    pub(crate) fn discard(&self, token: &str) -> AppResult<bool> {
        validate_preview_token(token)?;
        let mut cache = self.cache.lock_or_recover();
        purge_expired(&mut cache, Instant::now(), self.ttl);
        let Some(position) = cache.entries.iter().position(|entry| entry.token == token) else {
            return Ok(false);
        };
        let entry = cache.entries.remove(position).ok_or_else(preview_expired)?;
        cache.sensitive_bytes = cache.sensitive_bytes.saturating_sub(entry.sensitive_bytes);
        Ok(true)
    }

    fn insert_at(
        &self,
        envelope: ProviderShareEnvelopeV2,
        expected_final_name: String,
        expected_extensions: Vec<ProviderShareExtensionPreview>,
        source: PreviewSource,
        sensitive_bytes: usize,
        now: Instant,
    ) -> AppResult<String> {
        if self.max_entries == 0 || sensitive_bytes > self.max_sensitive_bytes {
            return Err(AppError::new(
                "PROVIDER_SHARE_PREVIEW_CAPACITY",
                "provider share preview cache capacity exceeded",
            ));
        }

        let mut cache = self.cache.lock_or_recover();
        purge_expired(&mut cache, now, self.ttl);
        while cache.entries.len() >= self.max_entries
            || cache
                .sensitive_bytes
                .checked_add(sensitive_bytes)
                .is_none_or(|total| total > self.max_sensitive_bytes)
        {
            let Some(evicted) = cache.entries.pop_front() else {
                return Err(AppError::new(
                    "PROVIDER_SHARE_PREVIEW_CAPACITY",
                    "provider share preview cache capacity exceeded",
                ));
            };
            cache.sensitive_bytes = cache
                .sensitive_bytes
                .saturating_sub(evicted.sensitive_bytes);
        }

        let token = generate_unique_token(&cache);
        cache.sensitive_bytes = cache
            .sensitive_bytes
            .checked_add(sensitive_bytes)
            .ok_or_else(|| {
                AppError::new(
                    "PROVIDER_SHARE_PREVIEW_CAPACITY",
                    "provider share preview cache capacity exceeded",
                )
            })?;
        cache.entries.push_back(PreviewEntry {
            token: token.clone(),
            created_at: now,
            sensitive_bytes,
            envelope,
            expected_final_name,
            expected_extensions,
            source,
        });
        let should_start_cleanup = !cache.cleanup_worker_running;
        if should_start_cleanup {
            cache.cleanup_worker_running = true;
        }
        drop(cache);
        if should_start_cleanup {
            spawn_cleanup_worker(Arc::clone(&self.cache), self.ttl);
        }
        Ok(token)
    }
}

pub(crate) struct PendingProviderShareImport {
    entry: PreviewEntry,
}

impl PendingProviderShareImport {
    pub(crate) fn verify_and_import(
        self,
        db: &db::Db,
    ) -> AppResult<crate::providers::ProviderSummary> {
        let Some(path) = self.entry.source.file_path.as_deref() else {
            return crate::providers::import_provider_share(
                db,
                &self.entry.envelope,
                &self.entry.expected_final_name,
                &self.entry.expected_extensions,
            );
        };
        let bytes = read_share_file(path).map_err(|_| preview_file_stale())?;
        if digest_bytes(&bytes) != self.entry.source.digest {
            return Err(preview_file_stale());
        }
        crate::providers::import_provider_share(
            db,
            &self.entry.envelope,
            &self.entry.expected_final_name,
            &self.entry.expected_extensions,
        )
    }
}

fn preview_from_draft(
    preview_token: String,
    draft: ProviderSharePreviewDraft,
) -> ProviderShareImportPreview {
    ProviderShareImportPreview {
        preview_token,
        cli_key: draft.cli_key,
        source_name: draft.source_name,
        final_name: draft.final_name,
        source_enabled: draft.source_enabled,
        import_enabled: false,
        auth_mode: draft.auth_mode,
        credential_status: draft.credential_status,
        extension_count: u32::try_from(draft.extensions.len()).unwrap_or(u32::MAX),
        extensions: draft.extensions,
        can_import: draft.can_import,
    }
}

fn read_share_file(path: &Path) -> AppResult<Vec<u8>> {
    crate::shared::fs::read_file_with_max_len(path, PROVIDER_SHARE_MAX_BYTES).map_err(|_| {
        AppError::new(
            "SEC_INVALID_INPUT",
            "selected provider share file could not be read or exceeds the maximum encoded size",
        )
    })
}

fn digest_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn generate_unique_token(cache: &PreviewCache) -> String {
    loop {
        let mut random = [0_u8; PREVIEW_TOKEN_BYTES];
        rand::thread_rng().fill_bytes(&mut random);
        let token = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if !cache.entries.iter().any(|entry| entry.token == token) {
            return token;
        }
    }
}

fn validate_preview_token(token: &str) -> AppResult<()> {
    if token.len() != PREVIEW_TOKEN_HEX_LEN || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share preview token is invalid",
        ));
    }
    Ok(())
}

fn purge_expired(cache: &mut PreviewCache, now: Instant, ttl: Duration) {
    let mut retained_bytes = 0_usize;
    cache.entries.retain(|entry| {
        let expired = now
            .checked_duration_since(entry.created_at)
            .is_some_and(|age| age >= ttl);
        if !expired {
            retained_bytes = retained_bytes.saturating_add(entry.sensitive_bytes);
        }
        !expired
    });
    cache.sensitive_bytes = retained_bytes;
}

fn spawn_cleanup_worker(cache: Arc<Mutex<PreviewCache>>, ttl: Duration) {
    tauri::async_runtime::spawn(async move {
        loop {
            let sleep_for = {
                let mut cache = cache.lock_or_recover();
                let now = Instant::now();
                purge_expired(&mut cache, now, ttl);
                let Some(next_wait) = cache
                    .entries
                    .iter()
                    .map(|entry| {
                        let age = now
                            .checked_duration_since(entry.created_at)
                            .unwrap_or_default();
                        ttl.saturating_sub(age)
                    })
                    .min()
                else {
                    cache.cleanup_worker_running = false;
                    return;
                };
                next_wait
            };

            if sleep_for.is_zero() {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(sleep_for).await;
            }
        }
    });
}

fn preview_expired() -> AppError {
    AppError::new(
        "PROVIDER_SHARE_PREVIEW_EXPIRED",
        "provider share preview expired; preview the share again",
    )
}

fn preview_file_stale() -> AppError {
    AppError::new(
        "PROVIDER_SHARE_PREVIEW_STALE",
        "selected provider share file changed or is unavailable; preview the share again",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> ProviderShareEnvelopeV2 {
        parse_provider_share(
            br#"{
  "type": "aio-coding-hub.provider-share",
  "schema_version": 1,
  "provider": {
    "cli_key": "claude",
    "name": "Cache Test",
    "enabled": true,
    "configuration": {
      "base_urls": ["https://example.invalid/v1"],
      "base_url_mode": "order",
      "priority": 100,
      "cost_multiplier": 1.0,
      "claude_models": {},
      "model_mapping": {},
      "availability_test_model": null,
      "limits": {
        "limit_5h_usd": null,
        "limit_daily_usd": null,
        "daily_reset_mode": "fixed",
        "daily_reset_time": "00:00:00",
        "limit_weekly_usd": null,
        "limit_monthly_usd": null,
        "limit_total_usd": null
      },
      "tags": [],
      "note": "",
      "bridge_type": null,
      "stream_idle_timeout_seconds": null,
      "upstream_retry_policy_override": null
    },
    "authentication": {"mode": "api_key", "api_key": "synthetic-key"},
    "extensions": []
  }
}"#,
        )
        .expect("parse test envelope")
    }

    fn source(seed: u8) -> PreviewSource {
        PreviewSource {
            digest: [seed; 32],
            file_path: None,
        }
    }

    #[test]
    fn preview_tokens_are_single_use_and_expire() {
        let service = ProviderShareService::with_limits(Duration::from_secs(2), 8, 1024);
        let now = Instant::now();
        let token = service
            .insert_at(
                envelope(),
                "Cache Test".to_string(),
                Vec::new(),
                source(1),
                100,
                now,
            )
            .expect("insert");
        assert_eq!(token.len(), PREVIEW_TOKEN_HEX_LEN);
        assert!(service.take_at(&token, now).is_ok());
        assert!(service.take_at(&token, now).is_err());

        let expired = service
            .insert_at(
                envelope(),
                "Cache Test".to_string(),
                Vec::new(),
                source(2),
                100,
                now,
            )
            .expect("insert expiring");
        let error = service
            .take_at(&expired, now + Duration::from_secs(2))
            .err()
            .expect("expired token must fail");
        assert_eq!(error.code(), "PROVIDER_SHARE_PREVIEW_EXPIRED");
    }

    #[tokio::test]
    async fn expiry_worker_releases_idle_sensitive_entries_without_another_cache_operation() {
        let service = ProviderShareService::with_limits(Duration::from_millis(20), 8, 1024);
        let token = service
            .insert_at(
                envelope(),
                "Idle".to_string(),
                Vec::new(),
                source(1),
                100,
                Instant::now(),
            )
            .expect("insert");
        {
            let cache = service.cache.lock_or_recover();
            assert!(cache.cleanup_worker_running);
            assert_eq!(cache.entries.len(), 1);
            assert_eq!(cache.entries[0].token, token);
        }

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let released = {
                    let cache = service.cache.lock_or_recover();
                    cache.entries.is_empty()
                        && cache.sensitive_bytes == 0
                        && !cache.cleanup_worker_running
                };
                if released {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("cleanup worker must release the idle preview by its TTL");
    }

    #[test]
    fn preview_capability_type_does_not_derive_debug() {
        let source = include_str!("provider_share_service.rs");
        let declaration = "pub(crate) struct ProviderShareImportPreview";
        let item_start = source.find(declaration).expect("preview type declaration");
        let attribute_start = source[..item_start]
            .rfind("\n\n")
            .map_or(0, |index| index + 2);
        assert!(!source[attribute_start..item_start].contains("Debug"));
    }

    #[test]
    fn preview_cache_evicts_oldest_for_entry_and_byte_limits() {
        let service = ProviderShareService::with_limits(Duration::from_secs(60), 2, 250);
        let now = Instant::now();
        let first = service
            .insert_at(
                envelope(),
                "One".to_string(),
                Vec::new(),
                source(1),
                100,
                now,
            )
            .expect("first");
        let second = service
            .insert_at(
                envelope(),
                "Two".to_string(),
                Vec::new(),
                source(2),
                100,
                now,
            )
            .expect("second");
        let third = service
            .insert_at(
                envelope(),
                "Three".to_string(),
                Vec::new(),
                source(3),
                100,
                now,
            )
            .expect("third");
        assert!(service.take_at(&first, now).is_err());
        assert!(service.take_at(&second, now).is_ok());
        assert!(service.take_at(&third, now).is_ok());

        let oversized = service.insert_at(
            envelope(),
            "Too Large".to_string(),
            Vec::new(),
            source(4),
            251,
            now,
        );
        assert!(oversized.is_err());
    }

    #[test]
    fn discard_is_idempotent_and_releases_sensitive_capacity() {
        let service = ProviderShareService::with_limits(Duration::from_secs(60), 2, 100);
        let now = Instant::now();
        let token = service
            .insert_at(
                envelope(),
                "Discard".to_string(),
                Vec::new(),
                source(1),
                100,
                now,
            )
            .expect("insert");
        assert!(service.discard(&token).expect("discard"));
        assert!(!service.discard(&token).expect("discard again"));
        service
            .insert_at(
                envelope(),
                "Replacement".to_string(),
                Vec::new(),
                source(2),
                100,
                now,
            )
            .expect("released capacity accepts replacement");
        assert_eq!(service.cache.lock_or_recover().sensitive_bytes, 100);
    }

    #[test]
    fn file_preview_import_fails_closed_when_file_changes_or_disappears() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-cache-file.db"))
            .expect("init db");
        let path = dir.path().join("provider.json");
        let original = b"original provider share bytes";
        std::fs::write(&path, original).expect("write original");
        let service = ProviderShareService::with_limits(Duration::from_secs(60), 8, 1024);
        let now = Instant::now();

        let changed_token = service
            .insert_at(
                envelope(),
                "Cache Test".to_string(),
                Vec::new(),
                PreviewSource {
                    digest: digest_bytes(original),
                    file_path: Some(path.clone()),
                },
                original.len(),
                now,
            )
            .expect("changed token");
        std::fs::write(&path, b"changed provider share bytes").expect("change file");
        let changed_error = service
            .take_at(&changed_token, now)
            .expect("take changed token")
            .verify_and_import(&db)
            .expect_err("changed file must fail");
        assert_eq!(changed_error.code(), "PROVIDER_SHARE_PREVIEW_STALE");

        std::fs::write(&path, original).expect("restore file");
        let missing_token = service
            .insert_at(
                envelope(),
                "Cache Test".to_string(),
                Vec::new(),
                PreviewSource {
                    digest: digest_bytes(original),
                    file_path: Some(path.clone()),
                },
                original.len(),
                now,
            )
            .expect("missing token");
        std::fs::remove_file(&path).expect("remove file");
        let missing_error = service
            .take_at(&missing_token, now)
            .expect("take missing token")
            .verify_and_import(&db)
            .expect_err("missing file must fail");
        assert_eq!(missing_error.code(), "PROVIDER_SHARE_PREVIEW_STALE");
        assert!(service.take_at(&missing_token, now).is_err());
        assert!(crate::providers::list_by_cli(&db, "claude")
            .expect("list providers")
            .is_empty());
    }
}
