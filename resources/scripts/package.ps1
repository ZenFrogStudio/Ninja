# Usage: ./resources/scripts/package.ps1 -VersionNumber 1.0.0
param(
  [Parameter(Mandatory=$true)]
  [string]$VersionNumber,

  # Sign with the local self-signed certificate created by
  # `dev-cert.ps1`, instead of the real one in Azure Key Vault. Needed
  # for a local build that actually launches: the WM requests UIAccess,
  # which Windows only grants to signed binaries.
  [switch]$DevSign
)

# Architectures to package. An entry is skipped when its Rust target
# isn't installed, so a machine set up for x64 only still produces a
# working x64 installer instead of failing partway through.
function InstalledArchitectures() {
  $candidates = [ordered]@{
    "x64"   = "x86_64-pc-windows-msvc"
    "arm64" = "aarch64-pc-windows-msvc"
  }

  $installed = (rustup target list --installed) -split "`n" | ForEach-Object { $_.Trim() }
  $result = [ordered]@{}

  foreach ($arch in $candidates.Keys) {
    if ($installed -contains $candidates[$arch]) {
      $result[$arch] = $candidates[$arch]
    } else {
      Write-Warning "Skipping $arch : target $($candidates[$arch]) is not installed."
      Write-Warning "  Add it with: rustup target add $($candidates[$arch])"
    }
  }

  if ($result.Count -eq 0) {
    Write-Output "ERROR: No Rust targets installed for packaging."
    Exit 1
  }

  return $result
}

function ExitOnError() {
  if ($LASTEXITCODE -ne 0) {
    Exit 1
  }
}

# Skipping code signing is only acceptable for local builds. In CI it must
# be a hard failure, otherwise a rotated or missing secret silently ships
# unsigned installers. Unsigned binaries also break UIAccess, which only
# works for signed executables in a secure location.
function SkipSigning() {
  param(
    [Parameter(Mandatory)]
    [string]$reason
  )

  if ($env:CI) {
    Write-Output "ERROR: Cannot sign in CI. $reason"
    Exit 1
  }

  Write-Output "Skipping signing (local build). $reason"
}

# Newest `signtool.exe` from the installed Windows SDKs.
function SignTool() {
  $found = Get-ChildItem `
    "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" `
    -ErrorAction SilentlyContinue |
    Sort-Object FullName |
    Select-Object -Last 1

  if (!$found) {
    Write-Output "ERROR: signtool.exe not found. Install the Windows SDK."
    Exit 1
  }

  return $found.FullName
}

# Signs with the certificate from `dev-cert.ps1`. Trusted on this machine
# only, so the result is for local testing and nothing else.
function DevSignFiles() {
  param(
    [Parameter(Mandatory)]
    [string[]]$filePaths
  )

  if ($env:CI) {
    Write-Output "ERROR: -DevSign must not be used in CI."
    Exit 1
  }

  $cert = Get-ChildItem "Cert:\CurrentUser\My" |
    Where-Object { $_.Subject -eq "CN=Ninja Development" -and $_.NotAfter -gt (Get-Date) } |
    Select-Object -First 1

  if (!$cert) {
    Write-Output "ERROR: No development certificate found."
    Write-Output "  Create one from an elevated prompt with:"
    Write-Output "    ./resources/scripts/dev-cert.ps1"
    Exit 1
  }

  Write-Output "Dev-signing $filePaths."

  # No timestamp: the certificate is local-only, so signatures expiring
  # with it is the correct behaviour.
  & (SignTool) sign /sha1 $cert.Thumbprint /fd sha256 $filePaths
  ExitOnError
}

$signingSecrets = @(
  "AZ_VAULT_URL",
  "AZ_CERT_NAME",
  "AZ_CLIENT_ID",
  "AZ_CLIENT_SECRET",
  "AZ_TENANT_ID",
  "RFC3161_TIMESTAMP_URL"
)

# Whether this build is able to sign anything. Checked before the build,
# not just before signing, because the UIAccess feature depends on it.
function SigningAvailable() {
  if ($DevSign) {
    $cert = Get-ChildItem "Cert:\CurrentUser\My" |
      Where-Object { $_.Subject -eq "CN=Ninja Development" -and $_.NotAfter -gt (Get-Date) }

    return [bool]$cert
  }

  if (!(Get-Command "azuresigntool" -ErrorAction SilentlyContinue)) {
    return $false
  }

  foreach ($secret in $signingSecrets) {
    if (!(Test-Path "env:$secret")) {
      return $false
    }
  }

  return $true
}

function SignFiles() {
  param(
    [Parameter(Mandatory)]
    [string[]]$filePaths
  )

  if ($DevSign) {
    DevSignFiles $filePaths
    Return
  }

  if (!(Get-Command "azuresigntool" -ErrorAction SilentlyContinue)) {
    SkipSigning "AzureSignTool is not installed."
    Return
  }

  foreach ($secret in $signingSecrets) {
    if (!(Test-Path "env:$secret")) {
      SkipSigning "Missing secret '$secret'."
      Return
    }
  }

  Write-Output "Signing $filePaths."
  azuresigntool sign -kvu $ENV:AZ_VAULT_URL `
    -kvc $ENV:AZ_CERT_NAME `
    -kvi $ENV:AZ_CLIENT_ID `
    -kvs $ENV:AZ_CLIENT_SECRET `
    -kvt $ENV:AZ_TENANT_ID `
    -tr $ENV:RFC3161_TIMESTAMP_URL `
    -td sha256 $filePaths

  ExitOnError
}

function BuildFrontend() {
  # The bar is vendored under `bar/`, so its assets are built here rather
  # than downloaded. Downloading would bundle upstream Zebar, which talks
  # to the WM over a websocket that no longer exists.
  #
  # No MSI comes out of this any more: the bar and the WM are one binary,
  # which the WiX package in `BuildInstallers` ships. Only the web assets
  # are produced, and `ninja.exe` embeds them.
  Write-Output "Building bar frontend"

  pnpm --dir bar install --frozen-lockfile
  ExitOnError

  # The desktop crate embeds the client API bundle via `include_str!`, so
  # both frontend packages must be built before cargo runs.
  pnpm --dir bar --filter ninja build
  ExitOnError
  pnpm --dir bar --filter @ninja/settings-ui build
  ExitOnError
}

function BuildExes() {
  $architectures = InstalledArchitectures
  $rustTargets = $architectures.Values

  # UIAccess is only granted by Windows to a binary whose signature chains
  # to a trusted root. Building it into an installer that cannot be signed
  # produces an app that fails to launch with "A referral was returned from
  # the server", so the feature tracks whether signing is available rather
  # than being unconditional.
  $uiAccess = SigningAvailable

  if (!$uiAccess) {
    Write-Warning 'Building without UIAccess: nothing is available to sign with.'
    Write-Warning '  Windows owned by elevated processes will not be moved or'
    Write-Warning '  resized. Everything else is unaffected.'
    Write-Warning '  To enable: ./resources/scripts/dev-cert.ps1 (elevated), then'
    Write-Warning '  re-run this script with -DevSign. See docs/signing.md.'
  }

  foreach ($target in $rustTargets) {
    $arch = ($architectures.GetEnumerator() | Where-Object { $_.Value -eq $target }).Key
    $outDir = "out/$arch"
    $sourceDir = "target/$target/release"

    $requiredExes = @("ninja.exe", "ninja-cli.exe", "ninja-watcher.exe")
    $sourcePaths = $requiredExes | ForEach-Object { "$sourceDir/$_" }

    # Always built rather than skipped when the binaries already exist:
    # the feature set can differ from the last run, and a stale UIAccess
    # binary would silently ship in an unsigned installer. Cargo no-ops
    # when nothing has changed.
    #
    # `ninja.exe` comes from the `ninja` package: the bar and the window
    # manager share one binary, and that package is the one that owns the
    # Tauri build. `-p wm` would produce only a library.
    Write-Output "Building executables for target '$target'"

    # `custom-protocol` is what makes Tauri serve the settings UI from the
    # assets embedded in the binary. Without it the app falls back to
    # `devUrl`, and the settings window shows "localhost refused to
    # connect" on any machine that isn't running the dev server. The Tauri
    # CLI passes this itself; a plain `cargo build` has to be told.
    # Qualified with the package name: several packages are selected here
    # and only `ninja` defines these features.
    $featureList = @("ninja/custom-protocol")
    if ($uiAccess) { $featureList += "ninja/ui_access" }

    cargo build --locked --release --target $target `
      -p ninja -p wm-cli -p wm-watcher `
      --features ($featureList -join ",")
    ExitOnError

    # Copied rather than moved: moving artifacts out of `target` forces
    # the next build to relink them, and a binary still held open by an
    # exited process can be read but not moved.
    Write-Output "Copying built executables from $sourceDir to $outDir"
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    Copy-Item -Force -Path $sourcePaths -Destination $outDir

    $outPaths = $requiredExes | ForEach-Object { "$outDir/$_" }
    SignFiles $outPaths
  }
}

function BuildInstallers() {
  $wixArchs = (InstalledArchitectures).Keys

  foreach ($arch in $wixArchs) {
    Write-Output "Creating WM MSI ($arch)"
    wix build -arch $arch -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
      -out "./out/ninja-wm-$arch.msi" "./resources/wix/standalone.wxs" "./resources/wix/standalone-ui.wxs" `
      -d VERSION_NUMBER="$VersionNumber" `
      -d EXE_DIR="out/$arch"
    ExitOnError
  }

  SignFiles ($wixArchs | ForEach-Object { "out/ninja-wm-$_.msi" })

  # `ninja-setup.exe` is the download users are given: it chains the WM and
  # bar MSIs for every architecture that was built. Architectures that were
  # skipped are compiled out of the chain, so a machine set up for x64 only
  # still gets a working setup rather than none at all.
  # The bar renders in WebView2, so setup carries the runtime bootstrapper
  # and installs it when the machine doesn't already have it. Embedded
  # rather than downloaded at install time: Burn needs a hash matching the
  # payload exactly, and Microsoft revises this download in place.
  $webView2Path = "out/MicrosoftEdgeWebview2Setup.exe"

  if (!(Test-Path $webView2Path)) {
    Write-Output "Downloading WebView2 bootstrapper"
    Invoke-WebRequest -Uri "https://go.microsoft.com/fwlink/p/?LinkId=2124703" `
      -OutFile $webView2Path
    ExitOnError
  }

  Write-Output "Creating setup ($($wixArchs -join ', '))"
  wix build -arch "x64" -ext WixToolset.BootstrapperApplications.wixext `
    -ext WixToolset.Util.wixext `
    -out "./out/unsigned-ninja-setup.exe" "./resources/wix/bundle.wxs" `
    -d VERSION_NUMBER="$VersionNumber" `
    -d INCLUDE_X64=$(if ($wixArchs -contains "x64") { "yes" } else { "no" }) `
    -d INCLUDE_ARM64=$(if ($wixArchs -contains "arm64") { "yes" } else { "no" })
  ExitOnError

  Write-Output "Detaching & reattaching Burn engine for signing"
  wix burn detach "./out/unsigned-ninja-setup.exe" -engine "./out/engine.exe"
  ExitOnError
  SignFiles @("out/engine.exe")

  wix burn reattach "./out/unsigned-ninja-setup.exe" `
    -engine "./out/engine.exe" `
    -o "./out/ninja-setup.exe"
  ExitOnError

  SignFiles @("out/ninja-setup.exe")

  # Leave only the artifacts meant to be run or shipped. The detached
  # engine and the pre-reattach bundle are byte-identical in appearance to
  # the real setup, so leaving them next to it invites running the wrong
  # one — which is how an unsigned, half-processed bundle got launched.
  Remove-Item -Force -ErrorAction SilentlyContinue `
    "./out/unsigned-ninja-setup.exe", `
    "./out/engine.exe", `
    "./out/unsigned-ninja-setup.wixpdb", `
    "./out/ninja-wm-x64.wixpdb", `
    "./out/ninja-wm-arm64.wixpdb"
}

function Package() {
  Write-Output "Packaging with version number: $VersionNumber"

  # Consumed by `env!("VERSION_NUMBER")` in both the WM and the bar, so it
  # has to be set before any cargo build.
  $env:VERSION_NUMBER = $VersionNumber

  # Read by the bar's `sign.ps1`, which Tauri invokes itself and so cannot
  # be passed `-DevSign` directly.
  if ($DevSign) {
    $env:NINJA_DEV_SIGN = "1"
  }

  Write-Output "Creating output directory"
  New-Item -ItemType Directory -Force -Path "out"

  BuildFrontend
  BuildExes
  BuildInstallers
}

Package
