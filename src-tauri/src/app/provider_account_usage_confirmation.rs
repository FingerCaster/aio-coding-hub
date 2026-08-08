//! Native confirmation for custom account-usage access to saved credentials.

use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

static CUSTOM_ACCOUNT_USAGE_CONFIRMATION_PERMITS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(1);

#[derive(Debug, Clone, Copy)]
pub(crate) enum CustomAccountUsageConfirmationKind {
    Enable,
    Test,
    Duplicate,
}

pub(crate) async fn confirm_custom_account_usage_network_access(
    app: &tauri::AppHandle,
    kind: CustomAccountUsageConfirmationKind,
    origins: &[String],
    permission_fingerprint: &str,
) -> Result<bool, String> {
    let _permit = CUSTOM_ACCOUNT_USAGE_CONFIRMATION_PERMITS
        .try_acquire()
        .map_err(|_| {
            "SEC_CONFIRM_BUSY: custom account usage confirmation already open".to_string()
        })?;
    let (title, message) = confirmation_content(kind, origins, permission_fingerprint);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "继续".to_string(),
            "取消".to_string(),
        ))
        .show(move |confirmed| {
            let _ = sender.send(confirmed);
        });
    receiver
        .await
        .map_err(|_| "SYSTEM_ERROR: custom account usage confirmation closed".to_string())
}

fn confirmation_content(
    kind: CustomAccountUsageConfirmationKind,
    origins: &[String],
    permission_fingerprint: &str,
) -> (&'static str, String) {
    let (title, action) = match kind {
        CustomAccountUsageConfirmationKind::Enable => (
            "确认启用自定义账户用量脚本",
            "自动账户用量查询将允许脚本使用该供应商已保存的 API Key。",
        ),
        CustomAccountUsageConfirmationKind::Test => (
            "确认测试自定义账户用量脚本",
            "本次测试将允许草稿脚本使用该供应商已保存的 API Key。",
        ),
        CustomAccountUsageConfirmationKind::Duplicate => (
            "确认复制自定义账户用量脚本",
            "复制后的供应商将获得一份新的本机授权，允许该脚本使用副本保存的 API Key。",
        ),
    };
    let targets = origins
        .iter()
        .map(|origin| format!("- {origin}"))
        .collect::<Vec<_>>()
        .join("\n");
    let message = format!(
        "{action}\n\n权限摘要（SHA-256）：\n{permission_fingerprint}\n\n脚本只能访问以下 HTTPS Origin：\n{targets}\n\n自定义脚本及全部目标都将被视为可信；它们可以读取或转发 API Key，应用无法验证其意图。仅在你已核对当前脚本、权限摘要和全部目标时继续。"
    );
    (title, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_identifies_the_exact_permission_and_trust_boundary() {
        let fingerprint = "a".repeat(64);
        let (_, message) = confirmation_content(
            CustomAccountUsageConfirmationKind::Enable,
            &["https://api.example.test".to_string()],
            &fingerprint,
        );

        assert!(message.contains(&fingerprint));
        assert!(message.contains("https://api.example.test"));
        assert!(message.contains("可以读取或转发 API Key"));

        let (_, changed_message) = confirmation_content(
            CustomAccountUsageConfirmationKind::Enable,
            &["https://api.example.test".to_string()],
            &"b".repeat(64),
        );
        assert_ne!(message, changed_message);
    }

    #[test]
    fn confirmation_slot_rejects_concurrent_dialogs_without_queueing() {
        let first = CUSTOM_ACCOUNT_USAGE_CONFIRMATION_PERMITS
            .try_acquire()
            .expect("first confirmation should acquire the slot");
        assert!(matches!(
            CUSTOM_ACCOUNT_USAGE_CONFIRMATION_PERMITS.try_acquire(),
            Err(tokio::sync::TryAcquireError::NoPermits)
        ));
        drop(first);
        assert!(CUSTOM_ACCOUNT_USAGE_CONFIRMATION_PERMITS
            .try_acquire()
            .is_ok());
    }
}
