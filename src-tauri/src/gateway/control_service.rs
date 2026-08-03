//! Usage: Gateway start and circuit-control orchestration.

use crate::{
    app::plugin_service, app::plugins::runtime_executor::RuntimeGatewayPluginExecutor,
    circuit_breaker, db, provider_circuit_breakers, providers, session_manager, settings,
};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use super::active_requests::ActiveRequestRegistry;
use super::background_tasks::GatewayBackgroundTasks;
use super::binder::{bind_exact, bind_first_available, resolve_gateway_binding};
use super::codex_session_id::CodexSessionIdCache;
use super::events::{GatewayLogEvent, GATEWAY_LOG_EVENT_NAME, GATEWAY_STATUS_EVENT_NAME};
use super::proxy::{GatewayErrorCode, ProviderBaseUrlPingCache, RecentErrorCache};
use super::routes::build_router;
use super::runtime::{GatewayAppState, GatewayRuntime, GatewayRuntimeInit};
use super::util::now_unix_seconds;
use super::GatewayProviderCircuitStatus;

pub(crate) struct GatewayStartResult {
    pub(crate) status: super::GatewayStatus,
    pub(crate) effective_preferred_port: u16,
}

pub(crate) struct GatewayControlService;

impl GatewayControlService {
    pub(crate) fn start(
        running: &mut Option<GatewayRuntime>,
        app: &tauri::AppHandle,
        db: db::Db,
        cfg: &settings::AppSettings,
        preferred_port: Option<u16>,
    ) -> crate::shared::error::AppResult<GatewayStartResult> {
        if let Some(runtime) = running.as_ref() {
            let status = runtime.status();
            let effective_preferred_port = status.port.unwrap_or(cfg.preferred_port);
            return Ok(GatewayStartResult {
                status,
                effective_preferred_port,
            });
        }

        let requested_port = preferred_port
            .filter(|port| *port > 0)
            .unwrap_or(cfg.preferred_port.max(settings::DEFAULT_GATEWAY_PORT));

        let binding = resolve_gateway_binding(cfg)?;
        let (port, std_listener) = if let Some(port) = binding.fixed_port {
            let listener = bind_exact(&binding.bind_host, port)?;
            (port, listener)
        } else {
            bind_first_available(&binding.bind_host, Some(requested_port))?
        };

        let listen_addr = super::listen::format_host_port(&binding.bind_host, port);
        let base_url = format!(
            "http://{}",
            super::listen::format_host_port(&binding.base_host, port)
        );
        let bind_addr = std_listener
            .local_addr()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], port)));

        emit_port_fallback_log(
            app,
            binding.fixed_port,
            requested_port,
            port,
            base_url.clone(),
        );
        configure_http_client(
            cfg,
            port,
            binding.bind_host.as_str(),
            binding.base_host.as_str(),
        )?;

        let background_tasks = GatewayBackgroundTasks::start(app.clone(), db.clone());
        let circuit = build_circuit_breaker(&db, cfg, background_tasks.circuit_persist_tx());
        let session = Arc::new(session_manager::SessionManager::new());
        let recent_errors = Arc::new(Mutex::new(RecentErrorCache::default()));
        let plugin_pipeline = load_gateway_plugin_pipeline(&db);
        let active_requests = Arc::new(ActiveRequestRegistry::default());

        let state = GatewayAppState {
            app: app.clone(),
            db: db.clone(),
            log_tx: background_tasks.log_tx(),
            circuit: circuit.clone(),
            session: session.clone(),
            codex_session_cache: Arc::new(Mutex::new(CodexSessionIdCache::default())),
            recent_errors: recent_errors.clone(),
            latency_cache: Arc::new(Mutex::new(ProviderBaseUrlPingCache::default())),
            plugin_pipeline: plugin_pipeline.clone(),
            #[cfg(test)]
            http_client_override: None,
            active_requests: active_requests.clone(),
        };
        let router = build_router(state);
        let (shutdown, shutdown_rx) = oneshot::channel::<()>();
        let task = tauri::async_runtime::spawn(async move {
            let listener = match tokio::net::TcpListener::from_std(std_listener) {
                Ok(listener) => listener,
                Err(err) => {
                    tracing::error!(
                        bind_addr = %bind_addr,
                        "gateway listener initialization failed: {}",
                        err
                    );
                    return;
                }
            };

            let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            });

            if let Err(err) = serve.await {
                tracing::error!(bind_addr = %bind_addr, "gateway server runtime error: {}", err);
            }
        });

        let runtime = GatewayRuntime::new(GatewayRuntimeInit {
            port,
            base_url,
            listen_addr,
            circuit,
            session,
            recent_errors,
            active_requests,
            shutdown,
            task,
            background_tasks,
            plugin_pipeline: plugin_pipeline.clone(),
        });
        let status = runtime.status();
        *running = Some(runtime);
        crate::app::heartbeat_watchdog::gated_emit(app, GATEWAY_STATUS_EVENT_NAME, &status);

        Ok(GatewayStartResult {
            status,
            effective_preferred_port: port,
        })
    }

    pub(crate) fn circuit_status(
        running: Option<&GatewayRuntime>,
        app: &tauri::AppHandle,
        db: &db::Db,
        cli_key: &str,
    ) -> crate::shared::error::AppResult<Vec<GatewayProviderCircuitStatus>> {
        let provider_ids = provider_ids_for_cli(db, cli_key)?;
        if provider_ids.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(runtime) = running {
            let now_unix = now_unix_seconds() as i64;
            return Ok(runtime.circuit_status(&provider_ids, now_unix));
        }

        let persisted = provider_circuit_breakers::load_all(db).unwrap_or_default();
        let cfg = settings::read(app)?;
        let failure_threshold = cfg.circuit_breaker_failure_threshold.max(1);

        Ok(stopped_circuit_statuses(
            provider_ids,
            &persisted,
            failure_threshold,
        ))
    }

    pub(crate) fn refresh_plugins(running: Option<&GatewayRuntime>, db: &db::Db) {
        let Some(runtime) = running else {
            return;
        };
        match plugin_service::enabled_plugins_for_gateway(db) {
            Ok(plugins) => {
                let plugin_count = plugins.len();
                runtime.refresh_plugin_pipeline(plugins);
                tracing::info!(plugin_count, "refreshed gateway plugin pipeline");
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to refresh gateway plugin pipeline; keeping previous snapshot"
                );
            }
        }
    }

    pub(crate) fn circuit_reset_provider(
        running: Option<&GatewayRuntime>,
        db: &db::Db,
        provider_id: i64,
    ) -> crate::shared::error::AppResult<()> {
        if provider_id <= 0 {
            return Err("SEC_INVALID_INPUT: provider_id must be > 0"
                .to_string()
                .into());
        }

        let now_unix = now_unix_seconds() as i64;
        if let Some(runtime) = running {
            let tombstones = runtime
                .circuit_reset_provider(provider_id, now_unix)
                .into_iter()
                .collect::<Vec<_>>();
            persist_reset_tombstones(db, &tombstones)?;
        } else {
            persist_closed_tombstones(db, &[provider_id], now_unix)?;
        }
        Ok(())
    }

    pub(crate) fn circuit_reset_cli(
        running: Option<&GatewayRuntime>,
        db: &db::Db,
        cli_key: &str,
    ) -> crate::shared::error::AppResult<usize> {
        let provider_ids = provider_ids_for_cli(db, cli_key)?;
        if provider_ids.is_empty() {
            return Ok(0);
        }

        let now_unix = now_unix_seconds() as i64;
        if let Some(runtime) = running {
            let tombstones = runtime.circuit_reset_cli(&provider_ids, now_unix);
            persist_reset_tombstones(db, &tombstones)?;
        } else {
            persist_closed_tombstones(db, &provider_ids, now_unix)?;
        }
        Ok(provider_ids.len())
    }
}

fn load_gateway_plugin_pipeline(
    db: &db::Db,
) -> Arc<super::plugins::pipeline::GatewayPluginPipeline> {
    match plugin_service::enabled_plugins_for_gateway(db) {
        Ok(plugins) => {
            if !plugins.is_empty() {
                tracing::info!(
                    plugin_count = plugins.len(),
                    "loaded enabled gateway plugins"
                );
            }
            Arc::new(
                super::plugins::pipeline::GatewayPluginPipeline::for_runtime(
                    plugins,
                    Arc::new(RuntimeGatewayPluginExecutor::with_db(db.clone())),
                    super::plugins::pipeline::GatewayPluginPipelineConfig::default(),
                ),
            )
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "failed to load gateway plugins; continuing with empty plugin pipeline"
            );
            empty_runtime_gateway_plugin_pipeline(db)
        }
    }
}

#[cfg(test)]
fn fallback_gateway_plugin_pipeline_for_tests(
    db: &db::Db,
) -> Arc<super::plugins::pipeline::GatewayPluginPipeline> {
    tracing::warn!("failed to load gateway plugins; continuing with empty plugin pipeline");
    empty_runtime_gateway_plugin_pipeline(db)
}

fn empty_runtime_gateway_plugin_pipeline(
    db: &db::Db,
) -> Arc<super::plugins::pipeline::GatewayPluginPipeline> {
    Arc::new(
        super::plugins::pipeline::GatewayPluginPipeline::for_runtime(
            Vec::new(),
            Arc::new(RuntimeGatewayPluginExecutor::with_db(db.clone())),
            super::plugins::pipeline::GatewayPluginPipelineConfig::default(),
        ),
    )
}

fn provider_ids_for_cli(db: &db::Db, cli_key: &str) -> crate::shared::error::AppResult<Vec<i64>> {
    Ok(providers::list_by_cli(db, cli_key)?
        .into_iter()
        .map(|provider| provider.id)
        .collect())
}

fn stopped_circuit_statuses(
    provider_ids: Vec<i64>,
    persisted: &std::collections::HashMap<i64, circuit_breaker::CircuitPersistedState>,
    failure_threshold: u32,
) -> Vec<GatewayProviderCircuitStatus> {
    provider_ids
        .into_iter()
        .map(|provider_id| {
            let Some(item) = persisted.get(&provider_id) else {
                return GatewayProviderCircuitStatus {
                    provider_id,
                    state: circuit_breaker::CircuitState::Closed.as_str().to_string(),
                    failure_count: 0,
                    failure_threshold,
                    open_until: None,
                    cooldown_until: None,
                };
            };

            let state = match item.state {
                circuit_breaker::CircuitState::HalfOpen => circuit_breaker::CircuitState::Open,
                state => state,
            };
            GatewayProviderCircuitStatus {
                provider_id,
                state: state.as_str().to_string(),
                failure_count: item.failure_timestamps.len().min(u32::MAX as usize) as u32,
                failure_threshold,
                open_until: item.open_until,
                cooldown_until: None,
            }
        })
        .collect()
}

fn persist_closed_tombstones(
    db: &db::Db,
    provider_ids: &[i64],
    now_unix: i64,
) -> crate::shared::error::AppResult<()> {
    let persisted = provider_circuit_breakers::load_all(db)?;
    let tombstones = provider_ids
        .iter()
        .copied()
        .filter(|provider_id| *provider_id > 0)
        .filter_map(|provider_id| {
            let current = persisted.get(&provider_id)?;
            Some(circuit_breaker::CircuitPersistedState {
                provider_id,
                state: circuit_breaker::CircuitState::Closed,
                failure_timestamps: Vec::new(),
                half_open_success_count: 0,
                open_until: None,
                probe_reference_at: None,
                next_probe_at: None,
                natural_probe_due_at: None,
                recovery_guard_until: None,
                state_revision: current.state_revision.saturating_add(1).max(1),
                updated_at: now_unix,
            })
        })
        .collect::<Vec<_>>();
    persist_reset_tombstones(db, &tombstones)?;
    Ok(())
}

fn persist_reset_tombstones(
    db: &db::Db,
    tombstones: &[circuit_breaker::CircuitPersistedState],
) -> crate::shared::error::AppResult<()> {
    provider_circuit_breakers::upsert_many_durable(db, tombstones)?;
    Ok(())
}

fn emit_port_fallback_log(
    app: &tauri::AppHandle,
    fixed_port: Option<u16>,
    requested_port: u16,
    bound_port: u16,
    base_url: String,
) {
    if fixed_port.is_none() && bound_port != requested_port {
        let payload = GatewayLogEvent {
            level: "warn",
            error_code: GatewayErrorCode::PortInUse.as_str(),
            message: format!("端口 {requested_port} 被占用，已自动切换到 {bound_port}"),
            requested_port,
            bound_port,
            base_url,
        };
        crate::app::heartbeat_watchdog::gated_emit(app, GATEWAY_LOG_EVENT_NAME, payload);
    }
}

fn configure_http_client(
    cfg: &settings::AppSettings,
    port: u16,
    bind_host: &str,
    base_host: &str,
) -> crate::shared::error::AppResult<()> {
    let context = super::http_client::runtime_self_check_context(port, bind_host, base_host);
    let proxy_url = if cfg.upstream_proxy_enabled {
        super::http_client::build_effective_proxy_url(
            Some(cfg.upstream_proxy_url.as_str()),
            Some(cfg.upstream_proxy_username.as_str()),
            Some(cfg.upstream_proxy_password.as_str()),
        )
        .map_err(|err| format!("{}: {err}", GatewayErrorCode::HttpClientInit.as_str()))?
    } else {
        None
    };
    super::http_client::validate_proxy_with_context(proxy_url.as_deref(), &context)
        .map_err(|err| format!("{}: {err}", GatewayErrorCode::HttpClientInit.as_str()))?;
    super::http_client::sync_runtime_context(port, bind_host, base_host);
    super::http_client::init(proxy_url.as_deref())
        .map_err(|err| format!("{}: {err}", GatewayErrorCode::HttpClientInit.as_str()).into())
}

fn build_circuit_breaker(
    db: &db::Db,
    cfg: &settings::AppSettings,
    persist_tx: tokio::sync::mpsc::Sender<circuit_breaker::CircuitPersistedState>,
) -> Arc<circuit_breaker::CircuitBreaker> {
    let circuit_initial = match provider_circuit_breakers::load_all(db) {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!("circuit breaker state load failed, using defaults: {}", err);
            Default::default()
        }
    };

    let circuit_config = circuit_breaker::CircuitBreakerConfig {
        failure_threshold: cfg.circuit_breaker_failure_threshold.max(1),
        open_duration_secs: (cfg.circuit_breaker_open_duration_minutes as i64).saturating_mul(60),
        provider_cooldown_secs: cfg.provider_cooldown_seconds as i64,
        natural_probe_max_wait_secs: cfg.natural_probe_max_wait_seconds as i64,
    };
    Arc::new(circuit_breaker::CircuitBreaker::new(
        circuit_config,
        circuit_initial,
        Some(persist_tx),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugin_contributions::PluginContributes;
    use crate::domain::plugins::{
        PluginDetail, PluginHook, PluginHostCompatibility, PluginInstallSource, PluginManifest,
        PluginPermissionRisk, PluginRuntime, PluginStatus, PluginSummary,
    };
    use crate::gateway::plugins::context::{GatewayPluginHookName, GatewayRequestHookInput};
    use axum::body::Bytes;
    use axum::http::{HeaderMap, Method};
    use rusqlite::params;
    use std::collections::BTreeMap;

    fn insert_circuit_test_provider(db: &db::Db, provider_id: i64) {
        let conn = db.open_connection().expect("open db");
        conn.execute(
            r#"
            INSERT INTO providers(
              id, provider_uuid, cli_key, name, base_url, api_key_plaintext,
              created_at, updated_at
            ) VALUES (?1, ?2, 'test-cli', ?3, 'https://example.test', '', 1, 1)
            "#,
            params![
                provider_id,
                format!("00000000-0000-4000-8000-{provider_id:012x}"),
                format!("provider-{provider_id}")
            ],
        )
        .expect("insert provider");
    }

    fn circuit_test_state(
        provider_id: i64,
        state: circuit_breaker::CircuitState,
        state_revision: u64,
        updated_at: i64,
    ) -> circuit_breaker::CircuitPersistedState {
        circuit_breaker::CircuitPersistedState {
            provider_id,
            state,
            failure_timestamps: if state == circuit_breaker::CircuitState::Open {
                vec![updated_at.max(0) as u64]
            } else {
                Vec::new()
            },
            half_open_success_count: 0,
            open_until: (state == circuit_breaker::CircuitState::Open)
                .then_some(updated_at.saturating_add(300)),
            probe_reference_at: None,
            next_probe_at: None,
            natural_probe_due_at: None,
            recovery_guard_until: None,
            state_revision,
            updated_at,
        }
    }

    #[test]
    fn configure_http_client_rejects_runtime_self_loop_proxy() {
        let cfg = settings::AppSettings {
            upstream_proxy_enabled: true,
            upstream_proxy_url: "http://127.0.0.1:37123".to_string(),
            ..settings::AppSettings::default()
        };

        let err = configure_http_client(&cfg, 37123, "127.0.0.1", "127.0.0.1")
            .expect_err("runtime self-loop proxy should be rejected")
            .to_string();

        assert!(err.contains(GatewayErrorCode::HttpClientInit.as_str()));
        assert!(err.contains("self-loop"));
    }

    #[test]
    fn stopped_status_keeps_expired_open_and_normalizes_persisted_half_open() {
        let persisted = std::collections::HashMap::from([
            (
                1,
                circuit_breaker::CircuitPersistedState {
                    provider_id: 1,
                    state: circuit_breaker::CircuitState::Open,
                    failure_timestamps: vec![1, 2],
                    half_open_success_count: 0,
                    open_until: Some(10),
                    probe_reference_at: Some(1),
                    next_probe_at: Some(2),
                    natural_probe_due_at: Some(3),
                    recovery_guard_until: None,
                    state_revision: 4,
                    updated_at: 1,
                },
            ),
            (
                2,
                circuit_breaker::CircuitPersistedState {
                    provider_id: 2,
                    state: circuit_breaker::CircuitState::HalfOpen,
                    failure_timestamps: vec![3],
                    half_open_success_count: 0,
                    open_until: None,
                    probe_reference_at: None,
                    next_probe_at: None,
                    natural_probe_due_at: None,
                    recovery_guard_until: None,
                    state_revision: 2,
                    updated_at: 2,
                },
            ),
        ]);

        let statuses = stopped_circuit_statuses(vec![1, 2, 3], &persisted, 5);

        assert_eq!(
            statuses[0].state,
            circuit_breaker::CircuitState::Open.as_str()
        );
        assert_eq!(statuses[0].open_until, Some(10));
        assert_eq!(statuses[0].failure_count, 2);
        assert_eq!(
            statuses[1].state,
            circuit_breaker::CircuitState::Open.as_str()
        );
        assert_eq!(
            statuses[2].state,
            circuit_breaker::CircuitState::Closed.as_str()
        );
    }

    #[test]
    fn cold_start_uses_non_default_natural_probe_max_wait() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&temp.path().join("gateway-circuit-config.db"))
            .expect("init db");
        let (persist_tx, _persist_rx) = tokio::sync::mpsc::channel(4);
        let cfg = settings::AppSettings {
            circuit_breaker_failure_threshold: 1,
            circuit_breaker_open_duration_minutes: 2,
            provider_cooldown_seconds: 17,
            natural_probe_max_wait_seconds: 47,
            ..settings::AppSettings::default()
        };

        let circuit = build_circuit_breaker(&db, &cfg, persist_tx);
        let opened = circuit.record_failure(99, 1_000, None).after;

        assert_eq!(opened.state, circuit_breaker::CircuitState::Open);
        assert_eq!(opened.next_probe_at, Some(1_017));
        assert_eq!(opened.natural_probe_due_at, Some(1_047));
        assert_eq!(opened.open_until, Some(1_120));
    }

    #[test]
    fn running_reset_tombstone_is_durable_before_late_open_and_reload() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&temp.path().join("gateway-running-reset.db"))
            .expect("init db");
        let provider_id = 21;
        insert_circuit_test_provider(&db, provider_id);

        let stale_open =
            circuit_test_state(provider_id, circuit_breaker::CircuitState::Open, 7, 70);
        provider_circuit_breakers::upsert_durable(&db, &stale_open).expect("persist queued open");

        let tombstone =
            circuit_test_state(provider_id, circuit_breaker::CircuitState::Closed, 8, 80);
        persist_reset_tombstones(&db, std::slice::from_ref(&tombstone))
            .expect("durably persist running reset");
        provider_circuit_breakers::upsert_durable(&db, &stale_open)
            .expect("late stale open is a successful no-op");

        let loaded = provider_circuit_breakers::load_all(&db).expect("immediate reload");
        let reloaded = circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            loaded,
            None,
        );
        let snapshot = reloaded.snapshot(provider_id, 81);
        assert_eq!(snapshot.state, circuit_breaker::CircuitState::Closed);
        assert_eq!(snapshot.state_revision, 8);
    }

    #[test]
    fn running_reset_durable_failure_is_returned() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&temp.path().join("gateway-running-reset-error.db"))
            .expect("init db");
        let missing_provider =
            circuit_test_state(999, circuit_breaker::CircuitState::Closed, 1, 10);

        let err = persist_reset_tombstones(&db, &[missing_provider])
            .expect_err("durable reset failure must reach the caller");

        assert_eq!(err.code(), "DB_ERROR");
    }

    #[test]
    fn healthy_no_state_resets_succeed_without_durable_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&temp.path().join("gateway-empty-reset.db"))
            .expect("init db");
        let provider_id = 31;
        insert_circuit_test_provider(&db, provider_id);
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let runtime = GatewayRuntime::for_tests(
            &rt,
            Arc::new(session_manager::SessionManager::new()),
            Arc::new(Mutex::new(RecentErrorCache::default())),
        );

        GatewayControlService::circuit_reset_provider(Some(&runtime), &db, provider_id)
            .expect("running healthy reset");
        GatewayControlService::circuit_reset_provider(None, &db, provider_id)
            .expect("stopped healthy reset");

        assert!(provider_circuit_breakers::load_all(&db)
            .expect("load empty reset state")
            .is_empty());
    }

    #[tokio::test]
    async fn fallback_gateway_plugin_pipeline_retains_runtime_executor_for_refresh() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db =
            crate::db::init_for_tests(&temp.path().join("gateway-fallback.db")).expect("init db");
        let pipeline = fallback_gateway_plugin_pipeline_for_tests(&db);

        pipeline.replace_plugins(vec![extension_host_plugin_without_root()]);

        let err = pipeline
            .run_request_hook(GatewayRequestHookInput {
                hook_name: GatewayPluginHookName::RequestAfterBodyRead,
                trace_id: "trace-fallback-executor".to_string(),
                cli_key: "codex".to_string(),
                method: Method::POST,
                path: "/v1/responses".to_string(),
                query: None,
                headers: HeaderMap::new(),
                body: Bytes::from_static(b"hello"),
                requested_model: None,
            })
            .await
            .expect_err("fallback pipeline should keep runtime executor after refresh");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_GATEWAY_FAILED");
        assert!(err
            .to_string()
            .contains("PLUGIN_EXTENSION_HOST_ROOT_UNAVAILABLE"));
    }

    fn extension_host_plugin_without_root() -> PluginDetail {
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: "example.extension".to_string(),
                name: "Example Extension".to_string(),
                current_version: Some("1.0.0".to_string()),
                status: PluginStatus::Enabled,
                runtime: "extensionHost".to_string(),
                permission_risk: PluginPermissionRisk::High,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: PluginManifest {
                id: "example.extension".to_string(),
                name: "Example Extension".to_string(),
                version: "1.0.0".to_string(),
                api_version: "1.0.0".to_string(),
                runtime: PluginRuntime::ExtensionHost {
                    language: "typescript".to_string(),
                },
                hooks: Vec::new(),
                permissions: Vec::new(),
                main: Some("dist/index.js".to_string()),
                activation_events: Vec::new(),
                contributes: Some(PluginContributes {
                    providers: Vec::new(),
                    protocols: Vec::new(),
                    protocol_bridges: Vec::new(),
                    commands: Vec::new(),
                    gateway_hooks: vec![PluginHook {
                        name: "gateway.request.afterBodyRead".to_string(),
                        priority: 10,
                        failure_policy: Some("fail-closed".to_string()),
                        timeout_ms: None,
                    }],
                    ui: BTreeMap::new(),
                }),
                capabilities: vec!["gateway.hooks".to_string()],
                host_compatibility: PluginHostCompatibility {
                    app: ">=0.56.0 <1.0.0".to_string(),
                    plugin_api: "^1.0.0".to_string(),
                    platforms: Vec::new(),
                },
                entry: None,
                config_schema: None,
                config_version: None,
                description: None,
                author: None,
                homepage: None,
                repository: None,
                license: None,
                checksum: None,
                signature: None,
                category: None,
            },
            install_source: PluginInstallSource::Local,
            installed_dir: None,
            config: serde_json::json!({}),
            granted_permissions: vec!["request.body.read".to_string()],
            pending_permissions: Vec::new(),
            audit_logs: Vec::new(),
            runtime_failures: Vec::new(),
            rollback_versions: Vec::new(),
        }
    }
}
