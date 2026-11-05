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

Push-Location src-tauri; cargo update -p kobold-client --offline 2>$null; Pop-Location
npm install --package-lock-only --no-audit --no-fund | Out-Null
git add package.json package-lock.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "Release $Version"
git tag "v$Version"
git push origin HEAD --tags
Write-Host "Pushed v$Version. The release workflow builds and publishes it."
