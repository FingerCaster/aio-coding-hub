//! Usage: Extension host stdio worker process.

use crate::domain::plugin_contributions::PluginContributes;
use crate::domain::plugins::PluginManifest;
use rquickjs::{CatchResultExt, CaughtError, Context, Function, Object, Runtime, Value as JsValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const WORKER_VERSION: u32 = 1;
const EXTENSION_HOST_JSON_RPC_BODY_EXPANSION_FACTOR: usize = 6;
const EXTENSION_HOST_JSON_RPC_OVERHEAD_BYTES: usize = 1024 * 1024;
const DEFAULT_JS_TIMEOUT_MS: u64 = 30_000;

pub(crate) fn default_extension_host_max_line_bytes() -> usize {
    crate::gateway::util::max_request_body_bytes()
        .saturating_mul(EXTENSION_HOST_JSON_RPC_BODY_EXPANSION_FACTOR)
        .saturating_add(EXTENSION_HOST_JSON_RPC_OVERHEAD_BYTES)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtensionHostWorkerConfig {
    pub(crate) plugin_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contribution_hash: Option<String>,
    #[serde(default = "default_max_line_bytes")]
    pub(crate) max_line_bytes: usize,
    #[serde(default = "default_js_timeout_ms")]
    pub(crate) startup_js_timeout_ms: u64,
    #[serde(default = "default_js_timeout_ms")]
    pub(crate) js_timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcErrorBody {
    code: i32,
    message: String,
    data: Value,
}

struct WorkerState {
    manifest: PluginManifest,
    expected_contribution_hash: Option<String>,
    manifest_contribution_hash: String,
    declared_commands: BTreeSet<String>,
    declared_gateway_hooks: BTreeSet<String>,
    context: Context,
    activated: bool,
    deadline: Arc<Mutex<Option<Instant>>>,
    startup_js_timeout: Duration,
    js_timeout: Duration,
    host_calls: Arc<Mutex<WorkerHostCallState>>,
}

struct WorkerHostCallState {
    next_host_call_id: u64,
    max_line_bytes: usize,
}

pub fn run_stdio_worker() {
    let result = run_stdio_worker_inner();
    if let Err(err) = result {
        let _ = writeln!(
            io::stderr(),
            "{}: {}",
            err.code,
            err.message.replace('\n', " ")
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
#[test]
fn extension_host_worker_process_entry_for_tests() {
    if !std::env::args().any(|arg| arg == "--extension-host-config") {
        return;
    }
    run_stdio_worker();
}

fn run_stdio_worker_inner() -> Result<(), WorkerError> {
    let config = read_config_from_args(std::env::args())?;
    if config.max_line_bytes == 0 {
        return Err(WorkerError::new(
            "PLUGIN_EXTENSION_HOST_INVALID_CONFIG",
            "maxLineBytes must be greater than zero",
        ));
    }
    let mut state = WorkerState::load(config.clone())?;

    emit_notification(
        "extension.ready",
        json!({ "workerVersion": WORKER_VERSION }),
        config.max_line_bytes,
    )?;

    loop {
        let line = {
            let stdin = io::stdin();
            let mut stdin = stdin.lock();
            read_bounded_stdin_line(&mut stdin, config.max_line_bytes)?
        };
        let line = match line {
            WorkerStdinLine::Line(line) => line,
            WorkerStdinLine::TooLarge => {
                emit_protocol_error(
                    Value::Null,
                    "PLUGIN_EXTENSION_HOST_REQUEST_TOO_LARGE",
                    format!(
                        "extension host request exceeded {} bytes",
                        config.max_line_bytes
                    ),
                    config.max_line_bytes,
                )?;
                continue;
            }
            WorkerStdinLine::Eof => break,
        };
        if line.is_empty() {
            continue;
        }
        let request: JsonRpcRequest = match serde_json::from_slice(&line) {
            Ok(request) => request,
            Err(err) => {
                emit_protocol_error(
                    Value::Null,
                    "PLUGIN_EXTENSION_HOST_PROTOCOL_ERROR",
                    format!("extension host request was not valid JSON-RPC: {err}"),
                    config.max_line_bytes,
                )?;
                continue;
            }
        };

        let id = request.id.clone();
        let result = state.handle_request(request);
        match result {
            Ok(value) => emit_result(id, value, config.max_line_bytes)?,
            Err(err) => emit_error(id, err, config.max_line_bytes)?,
        }
    }
    Ok(())
}

enum WorkerStdinLine {
    Line(Vec<u8>),
    TooLarge,
    Eof,
}

fn read_bounded_stdin_line(
    reader: &mut impl Read,
    max_line_bytes: usize,
) -> Result<WorkerStdinLine, WorkerError> {
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = reader.read(&mut byte).map_err(|err| {
            WorkerError::new(
                "PLUGIN_EXTENSION_HOST_READ_FAILED",
                format!("failed to read worker stdin: {err}"),
            )
        })?;
        if read == 0 {
            return if line.is_empty() {
                Ok(WorkerStdinLine::Eof)
            } else {
                Ok(WorkerStdinLine::Line(line))
            };
        }
        if byte[0] == b'\n' {
            return Ok(WorkerStdinLine::Line(line));
        }
        line.push(byte[0]);
        if line.len() > max_line_bytes {
            discard_stdin_line(reader)?;
            return Ok(WorkerStdinLine::TooLarge);
        }
    }
}

fn discard_stdin_line(reader: &mut impl Read) -> Result<(), WorkerError> {
    let mut byte = [0_u8; 1];
    loop {
        let read = reader.read(&mut byte).map_err(|err| {
            WorkerError::new(
                "PLUGIN_EXTENSION_HOST_READ_FAILED",
                format!("failed to discard oversized worker stdin line: {err}"),
            )
        })?;
        if read == 0 || byte[0] == b'\n' {
            return Ok(());
        }
    }
}

impl WorkerState {
    fn load(config: ExtensionHostWorkerConfig) -> Result<Self, WorkerError> {
        let manifest_path = config.plugin_root.join("plugin.json");
        let manifest: PluginManifest = read_json_file(&manifest_path)?;
        let manifest_contribution_hash = contribution_hash(&manifest);
        if config.contribution_hash.as_deref() != Some(manifest_contribution_hash.as_str()) {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_HANDSHAKE_FAILED",
                "extension host contribution hash did not match manifest on disk",
            ));
        }
        let main = manifest.main.as_deref().ok_or_else(|| {
            WorkerError::new(
                "PLUGIN_EXTENSION_HOST_INVALID_MANIFEST",
                "extensionHost manifest requires main",
            )
        })?;
        let main_path = resolve_child_path(&config.plugin_root, main)?;
        if !main_path.is_file() {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_MAIN_NOT_FOUND",
                format!(
                    "extension host main file does not exist: {}",
                    main_path.display()
                ),
            ));
        }
        let declared_commands = declared_commands(manifest.contributes.as_ref());
        let declared_gateway_hooks = declared_gateway_hooks(manifest.contributes.as_ref());
        let runtime = Runtime::new().map_err(js_init_error)?;
        runtime.set_memory_limit(32 * 1024 * 1024);
        runtime.set_max_stack_size(512 * 1024);
        let deadline = Arc::new(Mutex::new(None));
        let interrupt_deadline = Arc::clone(&deadline);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let deadline = interrupt_deadline
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            deadline.is_some_and(|deadline| Instant::now() >= deadline)
        })));
        let context = Context::full(&runtime).map_err(js_init_error)?;
        let mut state = Self {
            manifest,
            expected_contribution_hash: config.contribution_hash,
            manifest_contribution_hash,
            declared_commands,
            declared_gateway_hooks,
            context,
            activated: false,
            deadline,
            startup_js_timeout: Duration::from_millis(config.startup_js_timeout_ms),
            js_timeout: Duration::from_millis(config.js_timeout_ms),
            host_calls: Arc::new(Mutex::new(WorkerHostCallState {
                next_host_call_id: 1,
                max_line_bytes: config.max_line_bytes,
            })),
        };
        state.load_main(&main_path)?;
        Ok(state)
    }

    fn handle_request(&mut self, request: JsonRpcRequest) -> Result<Value, WorkerError> {
        match request.method.as_str() {
            "extension.handshake" => self.handshake(request.params),
            "extension.activate" => {
                self.activate()?;
                Ok(json!({ "activated": true }))
            }
            "extension.deactivate" => {
                self.deactivate()?;
                Ok(json!({ "deactivated": true }))
            }
            "commands.execute" => {
                let command = request
                    .params
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        WorkerError::new(
                            "PLUGIN_EXTENSION_HOST_INVALID_REQUEST",
                            "commands.execute requires command",
                        )
                    })?;
                let args = request.params.get("args").cloned().unwrap_or(Value::Null);
                self.execute_command(command, args)
            }
            "gatewayHooks.execute" => {
                let hook = request
                    .params
                    .get("hook")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        WorkerError::new(
                            "PLUGIN_EXTENSION_HOST_INVALID_REQUEST",
                            "gatewayHooks.execute requires hook",
                        )
                    })?;
                let context = request
                    .params
                    .get("context")
                    .cloned()
                    .unwrap_or(Value::Null);
                self.execute_gateway_hook(hook, context)
            }
            method => Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_METHOD_NOT_FOUND",
                format!("unsupported extension host method: {method}"),
            )),
        }
    }

    fn load_main(&mut self, main_path: &Path) -> Result<(), WorkerError> {
        let source = fs::read_to_string(main_path).map_err(|err| {
            WorkerError::new(
                "PLUGIN_EXTENSION_HOST_MAIN_READ_FAILED",
                format!("failed to read extension host main: {err}"),
            )
        })?;
        let escaped_source = serde_json::to_string(&source).map_err(|err| {
            WorkerError::new(
                "PLUGIN_EXTENSION_HOST_ENCODE_FAILED",
                format!("failed to encode extension host main: {err}"),
            )
        })?;
        let escaped_path =
            serde_json::to_string(&main_path.display().to_string()).map_err(|err| {
                WorkerError::new(
                    "PLUGIN_EXTENSION_HOST_ENCODE_FAILED",
                    format!("failed to encode extension host main path: {err}"),
                )
            })?;
        let bootstrap = format!(
            r#"
            globalThis.__aioCommands = Object.create(null);
            globalThis.__aioGatewayHooks = Object.create(null);
            globalThis.module = {{ exports: {{}} }};
            globalThis.exports = globalThis.module.exports;
            globalThis.__filename = {escaped_path};
            globalThis.__dirname = "";
            (function(module, exports) {{
              const require = function(name) {{
                throw new Error("PLUGIN_EXTENSION_HOST_REQUIRE_UNSUPPORTED: require is not available: " + name);
              }};
              const fn = new Function("module", "exports", "require", "__filename", "__dirname", {escaped_source});
              fn(module, exports, require, __filename, __dirname);
            }})(globalThis.module, globalThis.exports);
            "#
        );
        self.with_startup_js_deadline(|| {
            self.context.with(|ctx| {
                ctx.eval::<(), _>(bootstrap.as_str())
                    .catch(&ctx)
                    .map_err(|err| self.js_caught_error(err))
            })
        })
    }

    fn handshake(&self, params: Value) -> Result<Value, WorkerError> {
        let plugin_id = params.get("pluginId").and_then(Value::as_str);
        let version = params.get("version").and_then(Value::as_str);
        let api_version = params.get("apiVersion").and_then(Value::as_str);
        let contribution_hash = params.get("contributionHash").and_then(Value::as_str);
        if plugin_id != Some(self.manifest.id.as_str())
            || version != Some(self.manifest.version.as_str())
            || api_version != Some(self.manifest.api_version.as_str())
        {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_HANDSHAKE_FAILED",
                "extension host handshake metadata did not match manifest",
            ));
        }
        if self.expected_contribution_hash.as_deref() != contribution_hash {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_HANDSHAKE_FAILED",
                "extension host contribution hash did not match worker config",
            ));
        }
        if Some(self.manifest_contribution_hash.as_str()) != contribution_hash {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_HANDSHAKE_FAILED",
                "extension host contribution hash did not match manifest on disk",
            ));
        }
        Ok(json!({
            "pluginId": self.manifest.id,
            "version": self.manifest.version,
            "apiVersion": self.manifest.api_version,
            "workerVersion": WORKER_VERSION,
        }))
    }

    fn activate(&mut self) -> Result<(), WorkerError> {
        if self.activated {
            return Ok(());
        }
        let declared_commands = self.declared_commands.clone();
        let declared_gateway_hooks = self.declared_gateway_hooks.clone();
        let plugin_id = self.manifest.id.clone();
        let capabilities: BTreeSet<String> = self.manifest.capabilities.iter().cloned().collect();
        let host_calls = Arc::clone(&self.host_calls);
        self.with_startup_js_deadline(|| {
            self.context.with(|ctx| {
                let globals = ctx.globals();
                let api = Object::new(ctx.clone()).map_err(js_runtime_error)?;
                if capabilities.contains("commands.execute") {
                    let commands = Object::new(ctx.clone()).map_err(js_runtime_error)?;
                    let declared_for_register = declared_commands.clone();
                    let register = Function::new(
                        ctx.clone(),
                        move |command: String, handler: Function<'_>| -> rquickjs::Result<()> {
                            if !declared_for_register.contains(&command) {
                                return Err(rquickjs::Error::new_from_js_message(
                                    "command",
                                    "declared command",
                                    format!(
                                        "PLUGIN_EXTENSION_HOST_UNDECLARED_COMMAND: command {command} is not declared by manifest"
                                    ),
                                ));
                            }
                            let globals = handler.ctx().globals();
                            let registry: Object = globals.get("__aioCommands")?;
                            registry.set(command.as_str(), handler)
                        },
                    )
                    .map_err(js_runtime_error)?;
                    commands
                        .set("registerCommand", register)
                        .map_err(js_runtime_error)?;
                    api.set("commands", commands).map_err(js_runtime_error)?;
                }
                if capabilities.contains("gateway.hooks") {
                    let gateway = Object::new(ctx.clone()).map_err(js_runtime_error)?;
                    let declared_for_register = declared_gateway_hooks.clone();
                    let register = Function::new(
                        ctx.clone(),
                        move |hook: String, handler: Function<'_>| -> rquickjs::Result<()> {
                            if !declared_for_register.contains(&hook) {
                                return Err(rquickjs::Error::new_from_js_message(
                                    "gateway hook",
                                    "declared gateway hook",
                                    format!(
                                        "PLUGIN_EXTENSION_HOST_UNDECLARED_GATEWAY_HOOK: gateway hook {hook} is not declared by manifest"
                                    ),
                                ));
                            }
                            let globals = handler.ctx().globals();
                            let registry: Object = globals.get("__aioGatewayHooks")?;
                            registry.set(hook.as_str(), handler)
                        },
                    )
                    .map_err(js_runtime_error)?;
                    gateway.set("registerHook", register).map_err(js_runtime_error)?;
                    api.set("gateway", gateway).map_err(js_runtime_error)?;
                }

                let host_calls_for_api = Arc::clone(&host_calls);
                let host_call_fn = Function::new(
                    ctx.clone(),
                    move |method: String, params_json: String| -> rquickjs::Result<String> {
                        let params: Value = serde_json::from_str(&params_json).map_err(|err| {
                            rquickjs::Error::new_from_js_message(
                                "JSON string",
                                "host API params",
                                format!(
                                    "PLUGIN_EXTENSION_HOST_DECODE_FAILED: failed to decode host API params: {err}"
                                ),
                            )
                        })?;
                        let value = match host_call(&method, params, &host_calls_for_api) {
                            Ok(value) => json!({ "ok": true, "value": value }),
                            Err(err) => json!({
                                "ok": false,
                                "code": err.code,
                                "message": err.message,
                            }),
                        };
                        serde_json::to_string(&value).map_err(|err| {
                            worker_error_to_js(WorkerError::new(
                                "PLUGIN_EXTENSION_HOST_ENCODE_FAILED",
                                format!("failed to encode host API result: {err}"),
                            ))
                        })
                    },
                )
                .map_err(js_runtime_error)?;
                globals
                    .set("__aioHostCall", host_call_fn)
                    .map_err(js_runtime_error)?;
                ctx.eval::<(), _>(
                    r#"
                    globalThis.__aioHostApi = function(method, params) {
                      const response = JSON.parse(globalThis.__aioHostCall(
                        method,
                        JSON.stringify(params)
                      ));
                      if (!response || response.ok !== true) {
                        const code = response && response.code
                          ? response.code
                          : "PLUGIN_EXTENSION_HOST_API_ERROR";
                        const message = response && response.message
                          ? response.message
                          : "host API returned an error";
                        throw new Error(code + ": " + message);
                      }
                      return response.value;
                    };
                    "#,
                )
                .map_err(js_runtime_error)?;

                let plugin_id_json = serde_json::to_string(&plugin_id).map_err(|err| {
                    WorkerError::new(
                        "PLUGIN_EXTENSION_HOST_ENCODE_FAILED",
                        format!("failed to encode plugin id for host API: {err}"),
                    )
                })?;
                if capabilities.contains("storage.plugin") {
                    let storage_source = format!(
                        r#"
                        ({{
                          get(key) {{
                            return globalThis.__aioHostApi(
                              "storage.get",
                              {{ pluginId: {plugin_id_json}, key }}
                            );
                          }},
                          set(key, value) {{
                            globalThis.__aioHostApi(
                              "storage.set",
                              {{ pluginId: {plugin_id_json}, key, value }}
                            );
                          }}
                        }})
                        "#
                    );
                    let storage: Object =
                        ctx.eval(storage_source.as_str()).map_err(js_runtime_error)?;
                    api.set("storage", storage).map_err(js_runtime_error)?;
                }

                if capabilities.contains("diagnostics.read") {
                    let diagnostics_source = format!(
                        r#"
                        ({{
                          getRuntimeReports(limit) {{
                            return globalThis.__aioHostApi(
                              "diagnostics.getRuntimeReports",
                              {{ pluginId: {plugin_id_json}, limit }}
                            );
                          }}
                        }})
                        "#
                    );
                    let diagnostics: Object = ctx
                        .eval(diagnostics_source.as_str())
                        .map_err(js_runtime_error)?;
                    api.set("diagnostics", diagnostics)
                        .map_err(js_runtime_error)?;
                }

                if capabilities.contains("privacy.redact") {
                    let privacy_source = format!(
                        r#"
                        ({{
                          redactText(text, options) {{
                            return globalThis.__aioHostApi(
                              "privacy.redactText",
                              {{ pluginId: {plugin_id_json}, text, options: options || {{}} }}
                            );
                          }},
                          redactRequestBody(body, options) {{
                            return globalThis.__aioHostApi(
                              "privacy.redactRequestBody",
                              {{ pluginId: {plugin_id_json}, body, options: options || {{}} }}
                            );
                          }}
                        }})
                        "#
                    );
                    let privacy: Object =
                        ctx.eval(privacy_source.as_str()).map_err(js_runtime_error)?;
                    api.set("privacy", privacy).map_err(js_runtime_error)?;
                }

                let module: Object = globals.get("module").map_err(js_runtime_error)?;
                let exports: Object = module.get("exports").map_err(js_runtime_error)?;
                if !exports.contains_key("activate").map_err(js_runtime_error)? {
                    return Ok(());
                }
                let activate: Function = exports.get("activate").map_err(js_runtime_error)?;
                activate
                    .call::<_, ()>((api,))
                    .catch(&ctx)
                    .map_err(|err| self.js_caught_error(err))
            })
        })?;
        self.activated = true;
        Ok(())
    }

    fn deactivate(&mut self) -> Result<(), WorkerError> {
        if !self.activated {
            return Ok(());
        }
        self.with_startup_js_deadline(|| {
            self.context.with(|ctx| {
                let globals = ctx.globals();
                let module: Object = globals.get("module").map_err(js_runtime_error)?;
                let exports: Object = module.get("exports").map_err(js_runtime_error)?;
                if exports
                    .contains_key("deactivate")
                    .map_err(js_runtime_error)?
                {
                    let deactivate: Function =
                        exports.get("deactivate").map_err(js_runtime_error)?;
                    deactivate
                        .call::<_, ()>(())
                        .catch(&ctx)
                        .map_err(|err| self.js_caught_error(err))?;
                }
                Ok(())
            })
        })?;
        self.activated = false;
        Ok(())
    }

    fn execute_command(&mut self, command: &str, args: Value) -> Result<Value, WorkerError> {
        if !self.declared_commands.contains(command) {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_UNDECLARED_COMMAND",
                format!("command {command} is not declared by manifest"),
            ));
        }
        if !self
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability == "commands.execute")
        {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_FORBIDDEN",
                "extension host API requires commands.execute",
            ));
        }
        self.activate()?;
        let args_json = serde_json::to_string(&args).map_err(|err| {
            WorkerError::new(
                "PLUGIN_EXTENSION_HOST_ENCODE_FAILED",
                format!("failed to encode command args: {err}"),
            )
        })?;
        let command_name = command.to_string();
        self.with_js_deadline(|| {
            self.context.with(|ctx| {
                let globals = ctx.globals();
                let registry: Object = globals.get("__aioCommands").map_err(js_runtime_error)?;
                if !registry
                    .contains_key(command_name.as_str())
                    .map_err(js_runtime_error)?
                {
                    return Err(WorkerError::new(
                        "PLUGIN_EXTENSION_HOST_COMMAND_NOT_REGISTERED",
                        format!("command {command_name} was not registered during activation"),
                    ));
                }
                let handler: Function = registry
                    .get(command_name.as_str())
                    .map_err(js_runtime_error)?;
                let parsed_args: JsValue = ctx
                    .eval(format!("JSON.parse({})", json_string_literal(&args_json)).as_str())
                    .map_err(js_runtime_error)?;
                let result: JsValue = handler
                    .call((parsed_args,))
                    .catch(&ctx)
                    .map_err(|err| self.js_caught_error(err))?;
                let globals = ctx.globals();
                let json_obj: Object = globals.get("JSON").map_err(js_runtime_error)?;
                let stringify: Function = json_obj.get("stringify").map_err(js_runtime_error)?;
                let json_result: Option<String> =
                    stringify.call((result,)).map_err(js_runtime_error)?;
                let Some(json_result) = json_result else {
                    return Ok(Value::Null);
                };
                serde_json::from_str(&json_result).map_err(|err| {
                    WorkerError::new(
                        "PLUGIN_EXTENSION_HOST_DECODE_FAILED",
                        format!("command result was not JSON serializable: {err}"),
                    )
                })
            })
        })
    }

    fn execute_gateway_hook(&mut self, hook: &str, context: Value) -> Result<Value, WorkerError> {
        if !self.declared_gateway_hooks.contains(hook) {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_UNDECLARED_GATEWAY_HOOK",
                format!("gateway hook {hook} is not declared by manifest"),
            ));
        }
        if !self
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability == "gateway.hooks")
        {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_FORBIDDEN",
                "extension host API requires gateway.hooks",
            ));
        }
        self.activate()?;
        let context_json = serde_json::to_string(&context).map_err(|err| {
            WorkerError::new(
                "PLUGIN_EXTENSION_HOST_ENCODE_FAILED",
                format!("failed to encode gateway hook context: {err}"),
            )
        })?;
        let hook_name = hook.to_string();
        self.with_js_deadline(|| {
            self.context.with(|ctx| {
                let globals = ctx.globals();
                let registry: Object =
                    globals.get("__aioGatewayHooks").map_err(js_runtime_error)?;
                if !registry
                    .contains_key(hook_name.as_str())
                    .map_err(js_runtime_error)?
                {
                    return Err(WorkerError::new(
                        "PLUGIN_EXTENSION_HOST_GATEWAY_HOOK_NOT_REGISTERED",
                        format!("gateway hook {hook_name} was not registered during activation"),
                    ));
                }
                let handler: Function =
                    registry.get(hook_name.as_str()).map_err(js_runtime_error)?;
                let parsed_context: JsValue = ctx
                    .eval(format!("JSON.parse({})", json_string_literal(&context_json)).as_str())
                    .map_err(js_runtime_error)?;
                let result: JsValue = handler
                    .call((parsed_context,))
                    .catch(&ctx)
                    .map_err(|err| self.js_caught_error(err))?;
                let globals = ctx.globals();
                let json_obj: Object = globals.get("JSON").map_err(js_runtime_error)?;
                let stringify: Function = json_obj.get("stringify").map_err(js_runtime_error)?;
                let json_result: Option<String> =
                    stringify.call((result,)).map_err(js_runtime_error)?;
                let Some(json_result) = json_result else {
                    return Err(WorkerError::new(
                        "PLUGIN_EXTENSION_HOST_INVALID_OUTPUT",
                        "gateway hook result must be JSON serializable",
                    ));
                };
                serde_json::from_str(&json_result).map_err(|err| {
                    WorkerError::new(
                        "PLUGIN_EXTENSION_HOST_DECODE_FAILED",
                        format!("gateway hook result was not JSON serializable: {err}"),
                    )
                })
            })
        })
    }

    fn with_js_deadline<T>(
        &self,
        f: impl FnOnce() -> Result<T, WorkerError>,
    ) -> Result<T, WorkerError> {
        self.with_js_deadline_for(self.js_timeout, f)
    }

    fn with_startup_js_deadline<T>(
        &self,
        f: impl FnOnce() -> Result<T, WorkerError>,
    ) -> Result<T, WorkerError> {
        self.with_js_deadline_for(self.startup_js_timeout, f)
    }

    fn with_js_deadline_for<T>(
        &self,
        timeout: Duration,
        f: impl FnOnce() -> Result<T, WorkerError>,
    ) -> Result<T, WorkerError> {
        {
            let mut deadline = self
                .deadline
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *deadline = Some(Instant::now() + timeout);
        }
        let result = f();
        let mut deadline = self
            .deadline
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *deadline = None;
        result
    }

    fn js_caught_error(&self, err: CaughtError<'_>) -> WorkerError {
        self.js_error_message(err.to_string())
    }

    fn js_error_message(&self, message: String) -> WorkerError {
        let deadline_expired = self
            .deadline
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some_and(|deadline| Instant::now() >= deadline);
        if deadline_expired {
            return WorkerError::new(
                "PLUGIN_EXTENSION_HOST_TIMEOUT",
                "extension host JavaScript execution timed out",
            );
        }
        if message.contains("interrupted")
            || message.contains("interrupted by")
            || message.contains("InternalError: interrupted")
        {
            return WorkerError::new(
                "PLUGIN_EXTENSION_HOST_TIMEOUT",
                "extension host JavaScript execution timed out",
            );
        }
        if let Some((code, rest)) = split_error_code(&message) {
            return WorkerError::new(code, rest);
        }
        WorkerError::new(
            "PLUGIN_EXTENSION_HOST_JS_ERROR",
            format!("extension host JavaScript error: {message}"),
        )
    }
}

fn read_config_from_args(
    args: impl IntoIterator<Item = String>,
) -> Result<ExtensionHostWorkerConfig, WorkerError> {
    let mut args = args.into_iter();
    let mut config_path = None;
    while let Some(arg) = args.next() {
        if arg == "--extension-host-config" {
            config_path = args.next();
            break;
        }
    }
    let config_path = config_path.ok_or_else(|| {
        WorkerError::new(
            "PLUGIN_EXTENSION_HOST_INVALID_CONFIG",
            "--extension-host-config is required",
        )
    })?;
    read_json_file(Path::new(&config_path))
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, WorkerError> {
    let bytes = fs::read(path).map_err(|err| {
        WorkerError::new(
            "PLUGIN_EXTENSION_HOST_CONFIG_READ_FAILED",
            format!("failed to read {}: {err}", path.display()),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        WorkerError::new(
            "PLUGIN_EXTENSION_HOST_CONFIG_DECODE_FAILED",
            format!("failed to decode {}: {err}", path.display()),
        )
    })
}

fn declared_commands(contributes: Option<&PluginContributes>) -> BTreeSet<String> {
    contributes
        .map(|contributes| {
            contributes
                .commands
                .iter()
                .map(|command| command.command.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn declared_gateway_hooks(contributes: Option<&PluginContributes>) -> BTreeSet<String> {
    contributes
        .map(|contributes| {
            contributes
                .gateway_hooks
                .iter()
                .map(|hook| hook.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn contribution_hash(manifest: &PluginManifest) -> String {
    use sha2::Digest;

    let bytes = serde_json::to_vec(&json!({
        "runtime": manifest.runtime,
        "main": manifest.main,
        "activationEvents": manifest.activation_events,
        "contributes": manifest.contributes,
        "capabilities": manifest.capabilities,
        "permissions": manifest.permissions,
    }))
    .unwrap_or_default();
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn resolve_child_path(root: &Path, child: &str) -> Result<PathBuf, WorkerError> {
    let child_path = Path::new(child);
    if child_path.is_absolute()
        || child_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(WorkerError::new(
            "PLUGIN_EXTENSION_HOST_INVALID_MANIFEST",
            "extension host main must be a relative path inside the plugin root",
        ));
    }
    Ok(root.join(child_path))
}

fn emit_notification(
    method: &str,
    params: Value,
    max_line_bytes: usize,
) -> Result<(), WorkerError> {
    emit_line(
        json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
        max_line_bytes,
    )
}

fn emit_result(id: Value, result: Value, max_line_bytes: usize) -> Result<(), WorkerError> {
    emit_line(
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }),
        max_line_bytes,
    )
}

fn emit_error(id: Value, err: WorkerError, max_line_bytes: usize) -> Result<(), WorkerError> {
    emit_protocol_error(id, err.code, err.message, max_line_bytes)
}

fn emit_protocol_error(
    id: Value,
    code: impl Into<String>,
    message: impl Into<String>,
    max_line_bytes: usize,
) -> Result<(), WorkerError> {
    let message = message.into();
    emit_line(
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": JsonRpcErrorBody {
                code: -32000,
                message: message.clone(),
                data: json!({ "code": code.into() }),
            },
        }),
        max_line_bytes,
    )
}

fn emit_line(value: Value, max_line_bytes: usize) -> Result<(), WorkerError> {
    let mut bytes = serde_json::to_vec(&value).map_err(|err| {
        WorkerError::new(
            "PLUGIN_EXTENSION_HOST_ENCODE_FAILED",
            format!("failed to encode worker response: {err}"),
        )
    })?;
    if bytes.len() + 1 > max_line_bytes {
        bytes = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": value.get("id").cloned().unwrap_or(Value::Null),
            "error": {
                "code": -32000,
                "message": "extension host response exceeded max line bytes",
                "data": { "code": "PLUGIN_EXTENSION_HOST_RESPONSE_TOO_LARGE" }
            }
        }))
        .map_err(|err| {
            WorkerError::new(
                "PLUGIN_EXTENSION_HOST_ENCODE_FAILED",
                format!("failed to encode worker error response: {err}"),
            )
        })?;
    }
    if bytes.len() + 1 > max_line_bytes {
        return Err(WorkerError::new(
            "PLUGIN_EXTENSION_HOST_RESPONSE_TOO_LARGE",
            format!("extension host response exceeded {max_line_bytes} bytes"),
        ));
    }
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(&bytes).map_err(write_error)?;
    lock.write_all(b"\n").map_err(write_error)?;
    lock.flush().map_err(write_error)
}

fn host_call(
    method: &str,
    params: Value,
    host_calls: &Arc<Mutex<WorkerHostCallState>>,
) -> Result<Value, WorkerError> {
    let (id, max_line_bytes) = {
        let mut state = host_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = state.next_host_call_id;
        state.next_host_call_id = state.next_host_call_id.saturating_add(1);
        (id, state.max_line_bytes)
    };
    emit_line(
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "host.call",
            "params": {
                "method": method,
                "params": params,
            },
        }),
        max_line_bytes,
    )?;

    let line = {
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        read_bounded_stdin_line(&mut stdin, max_line_bytes)?
    };
    let line = match line {
        WorkerStdinLine::Line(line) => line,
        WorkerStdinLine::TooLarge => {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_RESPONSE_TOO_LARGE",
                format!("host API response exceeded {max_line_bytes} bytes"),
            ));
        }
        WorkerStdinLine::Eof => {
            return Err(WorkerError::new(
                "PLUGIN_EXTENSION_HOST_HOST_CLOSED",
                "host closed stdin before returning host API response",
            ));
        }
    };
    let response: Value = serde_json::from_slice(&line).map_err(|err| {
        WorkerError::new(
            "PLUGIN_EXTENSION_HOST_PROTOCOL_ERROR",
            format!("host API response was not valid JSON-RPC: {err}"),
        )
    })?;
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(WorkerError::new(
            "PLUGIN_EXTENSION_HOST_PROTOCOL_ERROR",
            "host API response must use JSON-RPC 2.0",
        ));
    }
    if response.get("id").and_then(Value::as_u64) != Some(id) {
        return Err(WorkerError::new(
            "PLUGIN_EXTENSION_HOST_PROTOCOL_ERROR",
            "host API response id did not match request id",
        ));
    }
    if let Some(error) = response.get("error") {
        let code = error
            .get("data")
            .and_then(|data| data.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("PLUGIN_EXTENSION_HOST_API_ERROR");
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("host API returned an error");
        return Err(WorkerError::new(code, message));
    }
    response.get("result").cloned().ok_or_else(|| {
        WorkerError::new(
            "PLUGIN_EXTENSION_HOST_PROTOCOL_ERROR",
            "host API response was missing result",
        )
    })
}

fn worker_error_to_js(err: WorkerError) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message(
        "host API",
        "successful host response",
        format!("{}: {}", err.code, err.message),
    )
}

fn json_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"null\"".to_string())
}

fn js_init_error(err: rquickjs::Error) -> WorkerError {
    WorkerError::new(
        "PLUGIN_EXTENSION_HOST_JS_INIT_FAILED",
        format!("failed to initialize extension host JavaScript runtime: {err}"),
    )
}

fn js_runtime_error(err: rquickjs::Error) -> WorkerError {
    let message = err.to_string();
    if message.contains("interrupted")
        || message.contains("interrupted by")
        || message.contains("InternalError: interrupted")
    {
        return WorkerError::new(
            "PLUGIN_EXTENSION_HOST_TIMEOUT",
            "extension host JavaScript execution timed out",
        );
    }
    if let Some((code, rest)) = split_error_code(&message) {
        return WorkerError::new(code, rest);
    }
    WorkerError::new(
        "PLUGIN_EXTENSION_HOST_JS_ERROR",
        format!("extension host JavaScript error: {message}"),
    )
}

fn split_error_code(raw: &str) -> Option<(&str, String)> {
    let message = raw.trim();
    if let Some(start) = message.find("PLUGIN_") {
        let code_and_rest = &message[start..];
        let (code, rest) = code_and_rest.trim().split_once(':')?;
        let code = code.trim();
        if is_plugin_error_code(code) {
            return Some((code, rest.trim().to_string()));
        }
    }
    let (_prefix, code_and_rest) = message.split_once(':').unwrap_or(("", message));
    let (code, rest) = code_and_rest.trim().split_once(':')?;
    let code = code.trim();
    if is_plugin_error_code(code) {
        return Some((code, rest.trim().to_string()));
    }
    None
}

fn is_plugin_error_code(code: &str) -> bool {
    code.starts_with("PLUGIN_")
        && code
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn write_error(err: io::Error) -> WorkerError {
    WorkerError::new(
        "PLUGIN_EXTENSION_HOST_WRITE_FAILED",
        format!("failed to write worker response: {err}"),
    )
}

fn default_max_line_bytes() -> usize {
    default_extension_host_max_line_bytes()
}

fn default_js_timeout_ms() -> u64 {
    DEFAULT_JS_TIMEOUT_MS
}

#[derive(Debug)]
struct WorkerError {
    code: String,
    message: String,
}

impl WorkerError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn worker_stdin_reader_rejects_oversized_line_without_buffering_remainder() {
        let mut input = Cursor::new([vec![b'x'; 128], b"\n{}".to_vec(), b"\n".to_vec()].concat());

        let first = read_bounded_stdin_line(&mut input, 64).expect("read oversized");
        assert!(matches!(first, WorkerStdinLine::TooLarge));

        let second = read_bounded_stdin_line(&mut input, 64).expect("read next line");
        match second {
            WorkerStdinLine::Line(line) => assert_eq!(line, b"{}"),
            WorkerStdinLine::TooLarge | WorkerStdinLine::Eof => {
                panic!("expected next valid line after oversized discard")
            }
        }
    }
}
