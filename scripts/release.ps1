<#
.SYNOPSIS
  Bumps the version everywhere, commits, tags and pushes. GitHub Actions then builds
  the installer and publishes the release that installed copies update from.

.EXAMPLE
  ./scripts/release.ps1 -Version 0.2.0
  ./scripts/release.ps1 -Version 0.2.0 -DryRun
#>
param(
  [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
  [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if ((git status --porcelain) -and -not $DryRun) { throw "Commit or stash your changes first." }
if (-not (Select-String -Path CHANGELOG.md -Pattern "^## \[?$([regex]::Escape($Version))\]?" -Quiet)) {
  throw "Add a '## $Version' section to CHANGELOG.md first."
}

function Set-Version($path, $pattern, $replacement) {
  $text = [IO.File]::ReadAllText("$root\$path")
  $new = [regex]::Replace($text, $pattern, $replacement, 1)
  if ($new -eq $text) { Write-Warning "$path unchanged" }
  if (-not $DryRun) { [IO.File]::WriteAllText("$root\$path", $new) }
}
Set-Version package.json '"version":\s*"[^"]+"' "`"version`": `"$Version`""
Set-Version src-tauri/tauri.conf.json '"version":\s*"[^"]+"' "`"version`": `"$Version`""
Set-Version src-tauri/Cargo.toml '(?m)^version = "[^"]+"' "version = `"$Version`""

if ($DryRun) { Write-Host "Dry run: would release v$Version"; return }

# Native tools print progress on stderr; Windows PowerShell would treat that as an
# error under 'Stop', so check exit codes instead.
$ErrorActionPreference = 'Continue'
function Invoke-Step([string]$what, [scriptblock]$cmd) {
  & $cmd 2>&1 | ForEach-Object { "$_" } | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "$what failed (exit $LASTEXITCODE)." }
}
Push-Location src-tauri
Invoke-Step 'cargo update' { cargo update -p kobold-client --offline }
Pop-Location
Invoke-Step 'npm lockfile update' { npm install --package-lock-only --no-audit --no-fund }
git add package.json package-lock.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
Invoke-Step 'git commit' { git commit -m "Release $Version" }
Invoke-Step 'git tag' { git tag -a "v$Version" -m "Kobold Client $Version" }
Invoke-Step 'git push' { git push origin HEAD "v$Version" }
Write-Host "Pushed v$Version. The release workflow builds and publishes it."
