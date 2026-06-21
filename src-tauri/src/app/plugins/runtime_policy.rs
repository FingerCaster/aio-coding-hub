//! Usage: Host policy flags for plugin runtime execution.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RuntimePolicy {
    pub(crate) wasm_enabled: bool,
    pub(crate) process_enabled: bool,
}
