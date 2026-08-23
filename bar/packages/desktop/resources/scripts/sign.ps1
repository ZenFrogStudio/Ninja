# Usage: ./resources/scripts/sign.ps1 -FilePath ninja-bar.exe
#
# Invoked by Tauri via `signCommand` in `tauri.conf.json`, so it takes no
# arguments beyond the file. `NINJA_DEV_SIGN` is set by `package.ps1
# -DevSign` to route local builds through the self-signed certificate.
param(
  [Parameter(Mandatory=$true)]
  [string]$FilePath
)

if ($env:NINJA_DEV_SIGN) {
  $cert = Get-ChildItem "Cert:\CurrentUser\My" |
    Where-Object { $_.Subject -eq "CN=Ninja Development" -and $_.NotAfter -gt (Get-Date) } |
    Select-Object -First 1

  if (!$cert) {
    Write-Output "ERROR: NINJA_DEV_SIGN is set but no development certificate exists."
    Exit 1
  }

  $signtool = Get-ChildItem `
    "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" `
    -ErrorAction SilentlyContinue |
    Sort-Object FullName |
    Select-Object -Last 1

  if (!$signtool) {
    Write-Output "ERROR: signtool.exe not found. Install the Windows SDK."
    Exit 1
  }

  Write-Output "Dev-signing $FilePath."
  & $signtool.FullName sign /sha1 $cert.Thumbprint /fd sha256 $FilePath

  if ($LASTEXITCODE -ne 0) {
    Exit 1
  }

  Return
}

if (!(Get-Command "azuresigntool" -ErrorAction SilentlyContinue)) {
  Write-Output "Skipping signing because AzureSignTool is not installed."
  Return
}

$secrets = @(
  "AZ_VAULT_URL",
  "AZ_CERT_NAME",
  "AZ_CLIENT_ID",
  "AZ_CLIENT_SECRET",
  "AZ_TENANT_ID",
  "RFC3161_TIMESTAMP_URL"
)

foreach ($secret in $secrets) {
  if (!(Test-Path "env:$secret")) {
    Write-Output "Skipping signing due to missing secret '$secret'."
    Return
  }
}

Write-Output "Signing $FilePath."
azuresigntool sign -kvu $ENV:AZ_VAULT_URL `
  -kvc $ENV:AZ_CERT_NAME `
  -kvi $ENV:AZ_CLIENT_ID `
  -kvs $ENV:AZ_CLIENT_SECRET `
  -kvt $ENV:AZ_TENANT_ID `
  -tr $ENV:RFC3161_TIMESTAMP_URL `
  -td sha256 $FilePath

if ($LASTEXITCODE -ne 0) {
  Exit 1
}
