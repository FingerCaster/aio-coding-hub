function arrayOption(value) {
  return Array.isArray(value)
    ? value.filter(function filterString(item) {
        return typeof item === "string";
      })
    : undefined;
}

function privacyOptions(config) {
  return {
    sensitiveTypes: arrayOption(config && config.sensitiveTypes),
    redactionScopes: arrayOption(config && config.redactionScopes),
  };
}

function handleRequestHook(api, payload) {
  const config = payload && payload.config ? payload.config : {};
  if (config.redactBeforeUpstream !== true) {
    return { action: "pass" };
  }
  const body =
    payload && payload.context && payload.context.request
      ? payload.context.request.body
      : undefined;
  if (typeof body !== "string" || body.length === 0) {
    return { action: "pass" };
  }
  const result = api.privacy.redactRequestBody(body, privacyOptions(config));
  return result && result.hit
    ? { action: "replace", requestBody: result.redacted }
    : { action: "pass" };
}

function handleLogHook(api, payload) {
  const config = payload && payload.config ? payload.config : {};
  if (config.redactLogs !== true) {
    return { action: "pass" };
  }
  const message =
    payload && payload.context && payload.context.log
      ? payload.context.log.message
      : undefined;
  if (typeof message !== "string" || message.length === 0) {
    return { action: "pass" };
  }
  const result = api.privacy.redactText(message, privacyOptions(config));
  return result && result.hit
    ? { action: "replace", logMessage: result.redacted }
    : { action: "pass" };
}

module.exports.activate = function activate(api) {
  api.gateway.registerHook(
    "gateway.request.afterBodyRead",
    function onAfterBodyRead(payload) {
      return handleRequestHook(api, payload);
    }
  );
  api.gateway.registerHook(
    "gateway.request.beforeSend",
    function onBeforeSend(payload) {
      return handleRequestHook(api, payload);
    }
  );
  api.gateway.registerHook("log.beforePersist", function onBeforePersist(payload) {
    return handleLogHook(api, payload);
  });
};
