use super::*;
const MAX_AGENT_BYTES: usize = 256 * 1024;

fn agents_dir(target: &NativeTarget) -> AppResult<PathBuf> {
    check_target(target, false)?;
    let path = Path::new(&target.agent_dir).join("agents");
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if targets::is_link(&meta) || !meta.is_dir() => {
            Err(invalid("OMP agents 目录不是普通目录"))
        }
        Ok(_) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(_) => Err(invalid("无法检查 OMP agents 目录")),
    }
}

fn agent_path(target: &NativeTarget, file_name: &str) -> AppResult<PathBuf> {
    let stem = file_name
        .strip_suffix(".md")
        .ok_or_else(|| invalid("Agent 文件必须使用 .md 扩展名"))?;
    let base = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    if stem.is_empty()
        || file_name.len() > 128
        || file_name.starts_with('.')
        || file_name
            .chars()
            .any(|c| c.is_control() || r#"/\:<>|?*""#.contains(c))
        || stem.ends_with([' ', '.'])
        || matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base.as_bytes()[3].is_ascii_digit())
    {
        return Err(invalid(
            "Agent 文件名无效；只能使用当前目录中的普通 Markdown 文件",
        ));
    }
    Ok(agents_dir(target)?.join(file_name))
}

fn agent_bytes(path: &Path) -> AppResult<Option<Vec<u8>>> {
    safe_file(path)?;
    crate::shared::fs::read_optional_file_with_max_len(path, MAX_AGENT_BYTES)
        .map_err(|_| invalid("Agent 文件无法读取或超过 256 KiB"))
}

fn agent_revision(path: &Path, bytes: Option<&[u8]>) -> String {
    if bytes.is_none() {
        "missing".into()
    } else {
        revision(path, bytes)
    }
}

fn parse_agent(content: &str, file_name: &str) -> AppResult<OmpAgentSummary> {
    if content.len() > MAX_AGENT_BYTES {
        return Err(invalid("Agent 文件超过 256 KiB"));
    }
    let text = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| invalid("Agent 需要以 YAML frontmatter（---）开头"))?;
    let end = rest
        .find("\n---\n")
        .or_else(|| rest.strip_suffix("\n---").map(str::len))
        .ok_or_else(|| invalid("Agent 的 YAML frontmatter 缺少结束分隔符"))?;
    if end > 64 * 1024 {
        return Err(invalid("Agent frontmatter 超过 64 KiB"));
    }
    let root = document::parse_mapping(NativeFormat::Yaml, &rest.as_bytes()[..end])
        .map_err(|_| invalid("Agent frontmatter 无效或包含不支持的 YAML 语法"))?;
    let name = root
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| valid_name(n) && !["main", "sub"].contains(&n.to_lowercase().as_str()))
        .ok_or_else(|| invalid("Agent 需要有效 name，且不能使用 main 或 sub"))?;
    let description = root
        .get("description")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= 8192)
        .ok_or_else(|| invalid("Agent 需要非空 description（最多 8192 字节）"))?;
    let model = match root.get("model") {
        None => Vec::new(),
        Some(Value::String(s)) if model_selector(&json!(s)) => {
            s.split(',').map(|s| s.trim().into()).collect()
        }
        Some(value) if model_selector(value) => value
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => return Err(invalid("Agent model 必须是模型选择器或非空选择器数组")),
    };
    let thinking = root
        .get("thinkingLevel")
        .or_else(|| root.get("thinking-level"))
        .or_else(|| root.get("thinking"));
    if thinking.is_some_and(|v| {
        !v.as_str()
            .is_some_and(|s| EFFORTS.contains(&s) || ["auto", "off"].contains(&s))
    }) {
        return Err(invalid("Agent 思考等级无效"));
    }
    for key in ["tools", "spawns", "autoloadSkills", "autoload-skills"] {
        if let Some(value) = root.get(key) {
            if !value.is_string()
                && !value
                    .as_array()
                    .is_some_and(|a| a.len() <= 256 && a.iter().all(Value::is_string))
            {
                return Err(invalid(
                    "Agent 工具、可派生 Agent 与技能列表必须为文本或字符串数组",
                ));
            }
        }
    }
    for key in ["blocking", "read-summarize", "readSummarize"] {
        if root.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(invalid("Agent 开关必须为 true 或 false"));
        }
    }
    for key in ["prewalk", "advisor"] {
        if root
            .get(key)
            .is_some_and(|v| !v.is_boolean() && !v.is_string())
        {
            return Err(invalid("Agent prewalk/advisor 必须为开关或模型选择器"));
        }
    }
    Ok(OmpAgentSummary {
        name: name.into(),
        description: description.into(),
        source: "custom".into(),
        file_name: Some(file_name.into()),
        model,
        thinking: thinking.and_then(Value::as_str).map(str::to_owned),
    })
}

pub(super) fn configured_agent(name: &str) -> OmpAgentSummary {
    OmpAgentSummary {
        name: name.into(),
        description: "仅发现配置覆盖；定义可能来自项目、插件或已移除的 Agent".into(),
        source: "configured".into(),
        file_name: None,
        model: vec![],
        thinking: None,
    }
}

pub(super) fn list_agents(
    target: &NativeTarget,
    warnings: &mut Vec<String>,
) -> AppResult<Vec<OmpAgentSummary>> {
    let mut result = Vec::new();
    let mut names = HashSet::new();
    let scan = || -> AppResult<Vec<PathBuf>> {
        let dir = agents_dir(target)?;
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(dir)
            .map_err(|_| invalid("无法读取 Agent 目录"))?
            .take(513)
        {
            let entry = entry.map_err(|_| invalid("无法读取 Agent 目录项目"))?;
            paths.push(entry.path());
        }
        if paths.len() > 512 {
            return Err(invalid("Agent 目录超过 512 个文件，请先整理"));
        }
        paths.sort();
        Ok(paths)
    };
    match scan() {
        Ok(paths) => {
            for path in paths {
                let Some(file_name) = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .filter(|s| s.ends_with(".md"))
                else {
                    continue;
                };
                let parsed = agent_path(target, file_name)
                    .and_then(|p| agent_bytes(&p))
                    .and_then(|b| {
                        let bytes = b.ok_or_else(conflict)?;
                        let text = std::str::from_utf8(&bytes)
                            .map_err(|_| invalid("Agent 编码不是 UTF-8"))?;
                        parse_agent(text, file_name)
                    });
                match parsed {
                Ok(agent) if names.insert(agent.name.clone()) => result.push(agent),
                Ok(_) => warnings.push(format!("Agent 文件 {file_name} 与其他自定义 Agent 重名，原生加载时先出现的定义生效。")),
                Err(_) => warnings.push(format!("Agent 文件 {file_name} 无法安全解析，本页不编辑该文件。")),
            }
            }
        }
        Err(_) => {
            warnings.push("无法安全读取自定义 Agent 目录，仅显示内置 Agent 和配置覆盖。".into())
        }
    }
    for (name, description, model, thinking) in [
        ("task", "通用多步骤任务", "@task", Some("auto")),
        ("sonic", "机械修改与数据收集", "@smol", Some("medium")),
        ("scout", "只读代码探索与调研", "@smol", Some("medium")),
        ("reviewer", "代码质量与安全审查", "@slow", None),
        ("security-reviewer", "只读安全漏洞审查", "", None),
    ] {
        if names.insert(name.into()) {
            result.push(OmpAgentSummary {
                name: name.into(),
                description: description.into(),
                source: "bundled".into(),
                file_name: None,
                model: if model.is_empty() {
                    vec![]
                } else {
                    vec![model.into()]
                },
                thinking: thinking.map(str::to_owned),
            });
        }
    }
    Ok(result)
}

pub(crate) fn read_agent(target: &NativeTarget, file_name: &str) -> AppResult<OmpAgentDocument> {
    let path = agent_path(target, file_name)?;
    let bytes = agent_bytes(&path)?;
    let content = bytes
        .as_ref()
        .map(|b| String::from_utf8(b.clone()).map_err(|_| invalid("Agent 编码不是 UTF-8")))
        .transpose()?
        .unwrap_or_default();
    if bytes.is_some() {
        parse_agent(&content, file_name)?;
    }
    Ok(OmpAgentDocument {
        target_id: target.target_id.clone(),
        file_name: file_name.into(),
        revision: agent_revision(&path, bytes.as_deref()),
        content,
    })
}

pub(crate) fn save_agent(
    target: &NativeTarget,
    input: &OmpAgentSaveInput,
) -> AppResult<OmpSettingsWriteResult> {
    check_target(target, true)?;
    let path = agent_path(target, &input.file_name)?;
    let parsed = parse_agent(&input.content, &input.file_name)?;
    // A new file cannot accidentally shadow an existing user definition.
    let mut warnings = Vec::new();
    if list_agents(target, &mut warnings)?.iter().any(|a| {
        a.source == "custom"
            && a.name == parsed.name
            && a.file_name.as_deref() != Some(&input.file_name)
    }) {
        return Err(invalid("当前目录已有同名 Agent，请编辑原文件"));
    }
    let current = agent_bytes(&path)?;
    let before = agent_revision(&path, current.as_deref());
    if before != input.expected_revision {
        return Err(conflict());
    }
    if current.as_deref() == Some(input.content.as_bytes()) {
        return Ok(OmpSettingsWriteResult {
            target_id: target.target_id.clone(),
            revision: before,
            changed: false,
            backup_path: None,
        });
    }
    let dir = agents_dir(target)?;
    files::create_directory(&dir)?;
    let backup = current
        .as_ref()
        .map(|b| files::backup(&path, b))
        .transpose()?;
    files::atomic_write(&path, input.content.as_bytes(), current.is_some(), || {
        check_target(target, true)?;
        if agent_path(target, &input.file_name)? != path
            || agent_revision(&path, agent_bytes(&path)?.as_deref()) != before
        {
            return Err(conflict());
        }
        Ok(())
    })?;
    Ok(OmpSettingsWriteResult {
        target_id: target.target_id.clone(),
        revision: agent_revision(&path, Some(input.content.as_bytes())),
        changed: true,
        backup_path: backup.map(|p| p.to_string_lossy().into()),
    })
}
