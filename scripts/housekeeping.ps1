<#
.SYNOPSIS
  Shows what takes space in the project folder and clears build output.
  Run without -Apply to only see sizes.

.EXAMPLE
  ./scripts/housekeeping.ps1
  ./scripts/housekeeping.ps1 -Apply
#>
param([switch]$Apply)
$root = Split-Path $PSScriptRoot -Parent

function Size($p) {
  if (-not (Test-Path $p)) { return 0 }
  (Get-ChildItem $p -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
}
function MB($b) { '{0,8:N0} MB' -f ($b / 1MB) }

$targets = @(
  @{ Name = 'Rust build output'; Path = "$root\src-tauri\target"; Clear = $true },
  @{ Name = 'UI build output';   Path = "$root\dist";             Clear = $true },
  @{ Name = 'node_modules';      Path = "$root\node_modules";     Clear = $false }
)
foreach ($t in $targets) { Write-Host ("{0} {1}" -f (MB (Size $t.Path)), $t.Name) }

if ($Apply) {
  Push-Location "$root\src-tauri"; cargo clean; Pop-Location
  Remove-Item "$root\dist" -Recurse -Force -ErrorAction SilentlyContinue
  Write-Host "Cleared build output. The next build starts from scratch."
}
