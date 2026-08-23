# Creates and trusts a self-signed code signing certificate for local
# builds, so that `package.ps1 -DevSign` produces an installer that
# actually runs on this machine.
#
# This exists because the WM is built with the `ui_access` feature, and
# Windows refuses to launch a UIAccess binary unless it is signed by a
# certificate chaining to a trusted root. An unsigned build fails at
# launch with "A referral was returned from the server".
#
# Run once, from an elevated prompt:
#
#   ./resources/scripts/dev-cert.ps1
#
# The certificate is trusted on this machine only. It is not a substitute
# for real signing: installers given to anyone else still need the `AZ_*`
# secrets.
#
#   ./resources/scripts/dev-cert.ps1 -Remove   removes it again

[CmdletBinding()]
param(
  # Remove the certificate from all three stores.
  [switch]$Remove
)

$ErrorActionPreference = 'Stop'

$subject = 'CN=Ninja Development'

# Trusting the root is what makes UIAccess accept the signature.
# `TrustedPublisher` additionally stops SmartScreen prompting on this
# machine.
$trustStores = @('Cert:\LocalMachine\Root', 'Cert:\LocalMachine\TrustedPublisher')

function AssertElevated() {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = [Security.Principal.WindowsPrincipal]$identity

  if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Output 'ERROR: This script must be run from an elevated prompt.'
    Write-Output '  Right-click Windows Terminal or PowerShell and choose'
    Write-Output '  "Run as administrator", then run it again.'
    Exit 1
  }
}

function RemoveCert() {
  $stores = @('Cert:\CurrentUser\My') + $trustStores

  foreach ($store in $stores) {
    Get-ChildItem $store |
      Where-Object { $_.Subject -eq $subject } |
      ForEach-Object {
        Write-Output "Removing $($_.Thumbprint) from $store"
        Remove-Item $_.PSPath -Force
      }
  }
}

function CreateCert() {
  $existing = Get-ChildItem 'Cert:\CurrentUser\My' |
    Where-Object { $_.Subject -eq $subject -and $_.NotAfter -gt (Get-Date) } |
    Select-Object -First 1

  if ($existing) {
    Write-Output "Reusing existing certificate: $($existing.Thumbprint)"
    return $existing
  }

  # Created in the user's store rather than the machine's so that
  # `package.ps1` can reach the private key without being elevated.
  Write-Output 'Creating self-signed code signing certificate'
  New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $subject `
    -KeyUsage DigitalSignature `
    -KeyAlgorithm RSA `
    -KeyLength 2048 `
    -NotAfter (Get-Date).AddYears(5) `
    -CertStoreLocation 'Cert:\CurrentUser\My'
}

function Trust() {
  param(
    [Parameter(Mandatory)]
    $cert
  )

  # Only the public half is copied into the machine stores; the private
  # key stays in the user store.
  $publicPath = Join-Path $env:TEMP 'ninja-dev-cert.cer'
  Export-Certificate -Cert $cert -FilePath $publicPath -Force | Out-Null

  try {
    foreach ($store in $trustStores) {
      Write-Output "Trusting certificate in $store"
      Import-Certificate -FilePath $publicPath -CertStoreLocation $store | Out-Null
    }
  } finally {
    Remove-Item $publicPath -Force -ErrorAction SilentlyContinue
  }
}

AssertElevated

if ($Remove) {
  RemoveCert
  Write-Output 'Done. Installers signed with it will no longer be trusted.'
  Exit 0
}

# Replace any prior copy so the machine stores never accumulate stale
# certificates with the same subject.
RemoveCert

$cert = CreateCert
Trust $cert

Write-Output ''
Write-Output "Certificate ready: $($cert.Thumbprint)"
Write-Output 'Build a signed installer with:'
Write-Output '  ./resources/scripts/package.ps1 -VersionNumber 0.1.0 -DevSign'
