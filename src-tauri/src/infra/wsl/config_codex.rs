//! WSL Codex CLI configuration.

use super::constants::{WSL_CODEX_API_KEY, WSL_CODEX_PROVIDER_KEY};
use super::shell::{bash_single_quote, run_wsl_bash_script, wsl_resolve_codex_home_script};

pub(super) fn configure_wsl_codex(
    distro: &str,
    proxy_origin: &str,
) -> crate::shared::error::AppResult<()> {
    let base_url = format!("{proxy_origin}/v1");
    let base_url = bash_single_quote(&base_url);
    let provider_key = bash_single_quote(WSL_CODEX_PROVIDER_KEY);
    let api_key = bash_single_quote(WSL_CODEX_API_KEY);

    let script = format!(
        r#"
set -euo pipefail

	HOME="$(getent passwd "$(whoami)" | cut -d: -f6)"
	export HOME

	{resolver}

	mkdir -p "$codex_home"
	config_path="$codex_home/config.toml"
	auth_path="$codex_home/auth.json"

if [ -L "$config_path" ]; then
  echo "Refusing to modify: $config_path is a symlink. Please manage it manually or remove the symlink first." >&2
  exit 2
fi
if [ -L "$auth_path" ]; then
  echo "Refusing to modify: $auth_path is a symlink. Please manage it manually or remove the symlink first." >&2
  exit 2
fi

base_url={base_url}
provider_key={provider_key}
api_key={api_key}

ts="$(date +%s)"
[ -f "$config_path" ] && cp -a "$config_path" "$config_path.bak.$ts"
[ -f "$auth_path" ] && cp -a "$auth_path" "$auth_path.bak.$ts"

tmp_config="$(mktemp "${{config_path}}.tmp.XXXXXX")"
tmp_auth="$(mktemp "${{auth_path}}.tmp.XXXXXX")"
cleanup() {{ rm -f "$tmp_config" "$tmp_auth"; }}
trap cleanup EXIT

if [ -s "$config_path" ]; then
  awk -v provider_key="$provider_key" -v base_url="$base_url" '
    BEGIN {{ in_root=1; seen_model=0; skipping=0 }}
    function ltrim(s) {{ sub(/^[[:space:]]+/, "", s); return s }}
    function rtrim(s) {{ sub(/[[:space:]]+$/, "", s); return s }}
    function extract_header(s) {{
      if (match(s, /^\[[^\]]+\]/)) {{
        return substr(s, RSTART, RLENGTH)
      }}
      return s
    }}
    function is_target_section(h, pk) {{
      header = extract_header(h)
      base1 = "[model_providers." pk "]"
      base2 = "[model_providers.\"" pk "\"]"
      base3 = "[model_providers.'"'"'" pk "'"'"']"
      prefix1 = "[model_providers." pk "."
      prefix2 = "[model_providers.\"" pk "\"."
      prefix3 = "[model_providers.'"'"'" pk "'"'"'."
      return (header == base1 || header == base2 || header == base3 || index(header, prefix1) == 1 || index(header, prefix2) == 1 || index(header, prefix3) == 1)
    }}
    {{
      line=$0
      trimmed=rtrim(ltrim(line))

      # skipping check BEFORE comment check to delete comments inside skipped section
      if (skipping) {{
        if (substr(trimmed, 1, 1) == "[") {{
          if (is_target_section(trimmed, provider_key)) {{
            next
          }}
          skipping=0
        }} else {{
          next
        }}
      }}

      if (trimmed ~ /^#/) {{ print line; next }}

      if (in_root && substr(trimmed, 1, 1) == "[") {{
        if (!seen_model) {{ print "model_provider = \"" provider_key "\""; print ""; seen_model=1 }}
        in_root=0
      }}

      if (is_target_section(trimmed, provider_key)) {{
        skipping=1
        next
      }}

      if (in_root && trimmed ~ /^preferred_auth_method[[:space:]]*=/) next
      if (in_root && trimmed ~ /^model_provider[[:space:]]*=/) {{
        if (!seen_model) {{ print "model_provider = \"" provider_key "\""; seen_model=1 }}
        next
      }}

      print line
    }}
    END {{
      if (in_root) {{
        if (!seen_model) print "model_provider = \"" provider_key "\""
      }}
      print ""
      print "[model_providers." provider_key "]"
      print "name = \"" provider_key "\""
      print "base_url = \"" base_url "\""
      print "wire_api = \"responses\""
      print "requires_openai_auth = true"
    }}
  ' "$config_path" > "$tmp_config"
else
  cat > "$tmp_config" <<EOF
model_provider = "$provider_key"

[model_providers.$provider_key]
name = "$provider_key"
base_url = "$base_url"
wire_api = "responses"
requires_openai_auth = true
EOF
fi

if [ ! -s "$tmp_config" ]; then
  echo "Sanity check failed: generated config.toml is empty" >&2
  exit 2
fi
grep -qF "model_provider = \"$provider_key\"" "$tmp_config" || {{ echo "Sanity check failed: missing model_provider" >&2; exit 2; }}
grep -qF "base_url = \"$base_url\"" "$tmp_config" || {{ echo "Sanity check failed: missing provider base_url" >&2; exit 2; }}

count_section="$(awk -v pk="$provider_key" '
  BEGIN {{ c=0 }}
  function extract_header(s) {{
    if (match(s, /^\[[^\]]+\]/)) {{
      return substr(s, RSTART, RLENGTH)
    }}
    return s
  }}
  {{
    line=$0
    sub(/^[[:space:]]+/, "", line)
    sub(/[[:space:]]+$/, "", line)
    if (line ~ /^#/) next
    if (substr(line, 1, 1) != "[") next
    header = extract_header(line)
    base1 = "[model_providers." pk "]"
    base2 = "[model_providers.\"" pk "\"]"
    base3 = "[model_providers.'"'"'" pk "'"'"']"
    if (header == base1 || header == base2 || header == base3) c++
  }}
  END {{ print c }}
' "$tmp_config")"
if [ "$count_section" -ne 1 ]; then
  echo "Sanity check failed: expected exactly one [model_providers.$provider_key] section, got $count_section" >&2
  exit 2
fi

if command -v jq >/dev/null 2>&1; then
  if [ -s "$auth_path" ]; then
    if ! jq -e 'type=="object"' "$auth_path" >/dev/null; then
      echo "Refusing to modify: $auth_path must be a JSON object." >&2
      exit 2
    fi
    jq --arg api_key "$api_key" '.OPENAI_API_KEY = $api_key' "$auth_path" > "$tmp_auth"
  else
    jq -n --arg api_key "$api_key" '{{OPENAI_API_KEY:$api_key}}' > "$tmp_auth"
  fi
  jq -e --arg api_key "$api_key" '.OPENAI_API_KEY == $api_key' "$tmp_auth" >/dev/null
elif command -v python3 >/dev/null 2>&1; then
  python3 - "$api_key" "$auth_path" "$tmp_auth" <<'PY'
import json
import sys
from pathlib import Path

api_key, src, dst = sys.argv[1], sys.argv[2], sys.argv[3]
data = {{}}
try:
    text = Path(src).read_text(encoding="utf-8")
    if text.strip():
        data = json.loads(text)
except FileNotFoundError:
    data = {{}}
except Exception as e:
    sys.stderr.write(f"Failed to parse existing auth.json: {{e}}\n")
    sys.exit(2)

if not isinstance(data, dict):
    sys.stderr.write("auth.json must be a JSON object\n")
    sys.exit(2)

data["OPENAI_API_KEY"] = api_key
Path(dst).write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY
else
  if [ -s "$auth_path" ]; then
    echo "Missing jq/python3; cannot safely merge existing $auth_path" >&2
    exit 2
  fi
  cat > "$tmp_auth" <<EOF
{{"OPENAI_API_KEY":"$api_key"}}
EOF
fi

if [ ! -s "$tmp_auth" ]; then
  echo "Sanity check failed: generated auth.json is empty" >&2
  exit 2
fi
grep -qF '"OPENAI_API_KEY"' "$tmp_auth" || {{ echo "Sanity check failed: missing OPENAI_API_KEY" >&2; exit 2; }}

if [ -f "$config_path" ]; then
  chmod --reference="$config_path" "$tmp_config" 2>/dev/null || true
fi
if [ -f "$auth_path" ]; then
  chmod --reference="$auth_path" "$tmp_auth" 2>/dev/null || true
fi

if mv -f "$tmp_config" "$config_path"; then
  if mv -f "$tmp_auth" "$auth_path"; then
    trap - EXIT
    exit 0
  fi

  echo "Failed to write $auth_path; attempting to rollback $config_path" >&2
  if [ -f "$config_path.bak.$ts" ]; then
    if cp -a "$config_path.bak.$ts" "$config_path"; then
      echo "Rollback successful: restored $config_path from backup" >&2
    else
      echo "CRITICAL: Rollback failed! Manual recovery needed: cp $config_path.bak.$ts $config_path" >&2
    fi
  else
    echo "WARNING: No backup found for $config_path, moving to $config_path.failed.$ts" >&2
    mv -f "$config_path" "$config_path.failed.$ts" 2>/dev/null || echo "CRITICAL: Failed to move config to .failed" >&2
  fi
  exit 1
fi

echo "Failed to write $config_path" >&2
exit 1
"#,
        resolver = wsl_resolve_codex_home_script("codex_home")
    );

    run_wsl_bash_script(distro, &script)
}
