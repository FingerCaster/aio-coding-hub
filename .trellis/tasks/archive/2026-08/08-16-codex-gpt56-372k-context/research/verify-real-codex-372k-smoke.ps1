[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$targetSlugs = @("gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna")
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$smokeHome = Join-Path $tempRoot ("aio-codex-372k-smoke-" + [guid]::NewGuid().ToString("N"))
$codex = Get-Command codex -ErrorAction Stop
$shell = (Get-Process -Id $PID).Path
$utf8NoBom = [Text.UTF8Encoding]::new($false)

function Invoke-IsolatedCodexModels {
    param([switch]$Bundled)

    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $shell
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.ArgumentList.Add("-NoProfile")
    $startInfo.ArgumentList.Add("-NonInteractive")
    $startInfo.ArgumentList.Add("-File")
    $startInfo.ArgumentList.Add($codex.Source)
    $startInfo.ArgumentList.Add("debug")
    $startInfo.ArgumentList.Add("models")
    if ($Bundled) {
        $startInfo.ArgumentList.Add("--bundled")
    }
    $startInfo.Environment["CODEX_HOME"] = $smokeHome

    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start Codex"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(30000)) {
        $process.Kill($true)
        throw "Codex debug models timed out"
    }
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) {
        throw "Codex debug models failed with exit code $($process.ExitCode): $stderr"
    }
    return $stdout | ConvertFrom-Json -Depth 100
}

function Assert-TargetWindows {
    param(
        [Parameter(Mandatory)]$Catalog,
        [Parameter(Mandatory)][long]$Expected
    )

    if ($null -eq $Catalog.models) {
        throw "Codex catalog has no models array"
    }
    foreach ($slug in $targetSlugs) {
        $matches = @($Catalog.models | Where-Object { $_.slug -ceq $slug })
        if ($matches.Count -ne 1) {
            throw "Expected exactly one $slug entry, found $($matches.Count)"
        }
        $model = $matches[0]
        if ([long]$model.context_window -ne $Expected) {
            throw "$slug context_window is $($model.context_window), expected $Expected"
        }
        if ([long]$model.max_context_window -ne $Expected) {
            throw "$slug max_context_window is $($model.max_context_window), expected $Expected"
        }
    }
}

try {
    [IO.Directory]::CreateDirectory($smokeHome) | Out-Null
    $bundled = Invoke-IsolatedCodexModels -Bundled
    Assert-TargetWindows -Catalog $bundled -Expected 272000

    foreach ($slug in $targetSlugs) {
        $model = @($bundled.models | Where-Object { $_.slug -ceq $slug })[0]
        $model.context_window = 372000
        $model.max_context_window = 372000
    }

    $catalogPath = Join-Path $smokeHome "managed-model-catalog.json"
    $catalogJson = $bundled | ConvertTo-Json -Depth 100 -Compress
    [IO.File]::WriteAllText($catalogPath, $catalogJson, $utf8NoBom)

    $catalogTomlString = $catalogPath | ConvertTo-Json -Compress
    $configPath = Join-Path $smokeHome "config.toml"
    [IO.File]::WriteAllText(
        $configPath,
        "model_catalog_json = $catalogTomlString`n",
        $utf8NoBom
    )

    $enabled = Invoke-IsolatedCodexModels
    Assert-TargetWindows -Catalog $enabled -Expected 372000

    [IO.File]::Delete($configPath)
    $disabled = Invoke-IsolatedCodexModels
    Assert-TargetWindows -Catalog $disabled -Expected 272000

    [pscustomobject]@{
        codex = (& $codex.Source --version)
        baseline = 272000
        enabled = 372000
        disabled = 272000
        targets = $targetSlugs
        isolatedHome = $smokeHome
    } | ConvertTo-Json -Depth 3
}
finally {
    if (Test-Path -LiteralPath $smokeHome) {
        $resolved = [IO.Path]::GetFullPath($smokeHome)
        $leaf = Split-Path -Leaf $resolved
        if (-not $resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to clean smoke path outside the system temp directory: $resolved"
        }
        if (-not $leaf.StartsWith("aio-codex-372k-smoke-", [StringComparison]::Ordinal)) {
            throw "Refusing to clean an unexpected smoke path: $resolved"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
