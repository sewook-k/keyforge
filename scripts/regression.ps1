[CmdletBinding()]
param(
  [switch]$BuildRelease,
  [switch]$RequireManual
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Invoke-Checked {
  param(
    [Parameter(Mandatory)][string]$Name,
    [Parameter(Mandatory)][string]$FilePath,
    [string[]]$Arguments = @()
  )

  Write-Host "`n== $Name ==" -ForegroundColor Cyan
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$Name failed with exit code $LASTEXITCODE."
  }
}

function Get-CargoPath {
  $command = Get-Command cargo -ErrorAction SilentlyContinue
  if ($null -ne $command) {
    return $command.Source
  }
  $fallback = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
  if (Test-Path -LiteralPath $fallback) {
    return $fallback
  }
  throw 'cargo was not found. Install Rust or add cargo to PATH.'
}

function Assert-VersionConsistency {
  param([string]$ProjectRoot)

  $packageVersion = (Get-Content -LiteralPath (Join-Path $ProjectRoot 'package.json') -Raw | ConvertFrom-Json).version
  $uiVersion = (Get-Content -LiteralPath (Join-Path $ProjectRoot 'apps\ui\package.json') -Raw | ConvertFrom-Json).version
  $tauriVersion = (Get-Content -LiteralPath (Join-Path $ProjectRoot 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
  $cargoToml = Get-Content -LiteralPath (Join-Path $ProjectRoot 'Cargo.toml') -Raw
  $cargoMatch = [regex]::Match($cargoToml, '(?ms)\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
  if (-not $cargoMatch.Success) {
    throw 'Could not read the workspace package version from Cargo.toml.'
  }
  $versions = @($packageVersion, $uiVersion, $tauriVersion, $cargoMatch.Groups[1].Value)
  if (($versions | Select-Object -Unique).Count -ne 1) {
    throw "Version mismatch: $($versions -join ', ')"
  }
  return $packageVersion
}

Push-Location $root
try {
  $cargo = Get-CargoPath
  $npm = (Get-Command npm -ErrorAction Stop).Source
  $version = Assert-VersionConsistency -ProjectRoot $root

  Write-Host "`n== Version manifest consistency ==" -ForegroundColor Cyan
  Write-Host "KeyForge $version"
  Invoke-Checked -Name 'Rust formatting' -FilePath $cargo -Arguments @('fmt', '--all', '--', '--check')
  Invoke-Checked -Name 'Rust lint' -FilePath $cargo -Arguments @('clippy', '--workspace', '--all-targets', '--all-features', '--', '-D', 'warnings')
  Invoke-Checked -Name 'Rust regression tests' -FilePath $cargo -Arguments @('test', '--workspace')
  Invoke-Checked -Name 'UI regression tests' -FilePath $npm -Arguments @('run', 'test:ui')
  Invoke-Checked -Name 'Production UI build' -FilePath $npm -Arguments @('run', 'build:ui')
  Invoke-Checked -Name 'Dependency audit' -FilePath $npm -Arguments @('audit', '--json')

  if ($BuildRelease) {
    $previousCi = $env:CI
    try {
      $env:CI = 'true'
      Invoke-Checked -Name 'Tauri Windows release build' -FilePath $npm -Arguments @('run', 'tauri', '--', 'build')
    }
    finally {
      if ($null -eq $previousCi) {
        Remove-Item Env:CI -ErrorAction SilentlyContinue
      }
      else {
        $env:CI = $previousCi
      }
    }
    $portable = Join-Path $root 'target\release\keyforge-app.exe'
    $installer = Join-Path $root "target\release\bundle\nsis\KeyForge_${version}_x64-setup.exe"
    foreach ($artifact in @($portable, $installer)) {
      if (-not (Test-Path -LiteralPath $artifact)) {
        throw "Expected release artifact was not produced: $artifact"
      }
      $fileVersion = (Get-Item -LiteralPath $artifact).VersionInfo.FileVersion
      $productVersion = (Get-Item -LiteralPath $artifact).VersionInfo.ProductVersion
      if ($fileVersion -ne $version -or $productVersion -ne $version) {
        throw "Version resource mismatch for ${artifact}: file=$fileVersion product=$productVersion expected=$version"
      }
    }
  }

  if ($RequireManual) {
    $manualResults = Join-Path $root 'docs\REGRESSION_MANUAL_RESULTS.md'
    $pending = Select-String -LiteralPath $manualResults -Pattern '\|\s*(PENDING|FAIL)\s*\|' -AllMatches
    if ($pending) {
      throw "Manual P0/P1 acceptance is incomplete. Record PASS with evidence in $manualResults before declaring a release complete."
    }
  }

  Write-Host "`nAUTOMATED REGRESSION GATE PASSED · KeyForge $version" -ForegroundColor Green
  if (-not $RequireManual) {
    Write-Host 'Physical Windows acceptance is still required for release completion; see docs\REGRESSION_MANUAL_RESULTS.md.' -ForegroundColor Yellow
  }
}
finally {
  Pop-Location
}
