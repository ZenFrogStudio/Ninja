# Starts Ninja (window manager + bar).
#
#   .\run.ps1              start the release build
#   .\run.ps1 -Dev         start the debug build, with consoles for logs
#   .\run.ps1 -Build       rebuild first, then start
#   .\run.ps1 -Stop        stop everything and exit
#
# Runs from the repo root regardless of where it's invoked from.

[CmdletBinding()]
param(
  # Use debug binaries. Slower, but they print logs to a console window.
  [switch]$Dev,

  # Rebuild before starting.
  [switch]$Build,

  # Stop any running instance and exit.
  [switch]$Stop,

  # Config file to use. Defaults to .testconfig if present, else the
  # user's own config at ~/.ninja/config.yaml.
  [string]$Config
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
Set-Location $root

# --- Make sure cargo is reachable -------------------------------------
# The rustup installer adds this to PATH, but terminals opened before the
# install won't have picked it up.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'

  if (Test-Path $cargoBin) {
    $env:Path += ";$cargoBin"
  }
}

function Stop-Ninja {
  # Ask the WM to exit cleanly first: that restores any hidden windows,
  # takes the bar down with it, and lets the watcher shut down properly.
  # Force-killing leaves all of them in a bad state.
  $cli = Join-Path $root 'target\release\ninja-cli.exe'

  if ((Test-Path $cli) -and (Get-Process ninja -ErrorAction SilentlyContinue)) {
    Write-Host 'Stopping Ninja...'
    & $cli command wm-exit *> $null
    Start-Sleep -Seconds 2
  }

  foreach ($name in @('ninja', 'ninja-watcher')) {
    Get-Process $name -ErrorAction SilentlyContinue |
      Stop-Process -Force -ErrorAction SilentlyContinue
  }

  Start-Sleep -Milliseconds 500
}

if ($Stop) {
  Stop-Ninja
  Write-Host 'Stopped.' -ForegroundColor Green
  exit 0
}

$profileDir = if ($Dev) { 'debug' } else { 'release' }
$binDir = Join-Path $root "target\$profileDir"

# --- Build ------------------------------------------------------------
if ($Build) {
  Write-Host "Building ($profileDir)..." -ForegroundColor Cyan

  # The bar embeds the client API bundle at compile time, so the frontend
  # has to be built before cargo runs.
  Push-Location (Join-Path $root 'bar')
  pnpm --filter ninja build
  if ($LASTEXITCODE -ne 0) { Pop-Location; throw 'Client API build failed.' }

  pnpm --filter '@ninja/settings-ui' build
  if ($LASTEXITCODE -ne 0) { Pop-Location; throw 'Settings UI build failed.' }
  Pop-Location

  # Binaries are locked while running, so stop first.
  Stop-Ninja

  # `ninja` hosts both the bar and the window manager, so this is the
  # only application binary. `custom-protocol` makes it load the built
  # settings UI; without it Tauri falls back to a dev server that isn't
  # running, and the settings window shows "localhost refused to
  # connect". Debug builds are meant to use the dev server, so the
  # feature is release-only.
  #
  # Note: splatting an empty array at a native command misbehaves in
  # Windows PowerShell, so the profiles are branched explicitly.
  if ($Dev) {
    cargo build -p ninja -p wm-cli
  } else {
    cargo build --release -p ninja -p wm-cli --features ninja/custom-protocol
  }

  if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }

  # The watcher is a background helper that outlives a crashed WM, so its
  # binary can stay locked by a process that didn't shut down cleanly.
  # That shouldn't block everything else from building.
  if ($Dev) {
    cargo build -p wm-watcher
  } else {
    cargo build --release -p wm-watcher
  }

  if ($LASTEXITCODE -ne 0) {
    Write-Warning 'Watcher build failed, likely locked by a running process.'
    Write-Warning 'The existing watcher binary will be used. Reboot to clear it.'
  }
}

# --- Check binary -----------------------------------------------------
$exe = Join-Path $binDir 'ninja.exe'

if (-not (Test-Path $exe)) {
  throw "Missing $exe. Run: .\run.ps1 -Build$(if ($Dev) { ' -Dev' })"
}

# --- Resolve config ---------------------------------------------------
if (-not $Config) {
  $testConfig = Join-Path $root '.testconfig\config.yaml'
  if (Test-Path $testConfig) { $Config = $testConfig }
}

Stop-Ninja

# --- Start ------------------------------------------------------------
Write-Host "Starting Ninja ($profileDir)..." -ForegroundColor Cyan

# `start` is the window manager's subcommand; the bar comes up alongside
# it in the same process.
$exeArgs = @('start')
if ($Config) {
  $exeArgs += @('--config', $Config)
  Write-Host "  config: $Config" -ForegroundColor DarkGray
}

if ($Dev) {
  Start-Process -FilePath $exe -ArgumentList $exeArgs
} else {
  Start-Process -FilePath $exe -ArgumentList $exeArgs -WindowStyle Hidden
}

Write-Host ''
Write-Host 'Ninja is running.' -ForegroundColor Green
Write-Host '  Stop with:  .\run.ps1 -Stop' -ForegroundColor DarkGray

if (-not $Dev) {
  Write-Host '  Logs: ~\.ninja\bar\errors.log.<date>. For a console: .\run.ps1 -Dev' -ForegroundColor DarkGray
}
