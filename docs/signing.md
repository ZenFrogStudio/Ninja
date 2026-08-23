# Code signing

Ninja needs a code signing certificate for one hard reason and one soft
one.

**Hard:** the WM requests UIAccess in its manifest
(`packages/wm/build.rs`), and Windows only grants that to a binary which
is both signed and installed in a secure location. An unsigned build with
`uiAccess="true"` does not start at all — it fails with "A referral was
returned from the server", which is Windows saying it does not trust the
binary.

**Soft:** unsigned installers trigger a SmartScreen warning on any machine
but the one that built them.

## The signature check cannot be waived

There are two gates. The secure-location gate is controlled by the "Only
elevate UIAccess applications that are installed in secure locations"
policy and can be turned off. The signature gate cannot: Microsoft's
documentation states the check runs "regardless of the state of this
security setting", and there is no end-user prompt to click through.

The reasoning is that a UIAccess process can drive the UAC dialog itself,
so it has to be trusted before it runs.

Two consequences worth stating plainly, because both come up:

- **Changing installer technology does not help.** The check happens when
  `ninja.exe` starts, not when it is installed. Inno Setup, NSIS, MSI and
  a plain zip all hit the same wall.
- **Users cannot opt in.** Unlike SmartScreen, there is no "run anyway".

Ref: [User Account Control: Only elevate UIAccess applications that are
installed in secure locations](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/security-policy-settings/user-account-control-only-elevate-uiaccess-applications-that-are-installed-in-secure-locations)

## What is lost without it

Only one thing: windows owned by processes running as administrator (Task
Manager, regedit, an elevated terminal) cannot be moved or resized, so
they sit unmanaged while everything else tiles.

Focus and window switching are unaffected — `NativeWindow::focus` beats
the foreground lock by sending an input to its own process first, which
needs no privilege.

Failures are logged and skipped rather than fatal, so the only symptom is
repeated "Failed to set window position" warnings in
`~/.ninja/errors.log`.

`package.ps1` compiles the feature out when it has nothing to sign with,
so an unsigned build is always launchable. The WM logs which mode it is in
at startup.

## Local builds

A self-signed certificate, trusted on the build machine only. Enough to
develop and test against a real install.

Once, from an **elevated** PowerShell:

```powershell
cd <repo root>
.\resources\scripts\dev-cert.ps1
```

Then build normally with the switch:

```powershell
.\resources\scripts\package.ps1 -VersionNumber 0.1.0 -DevSign
```

`SigningAvailable` in `package.ps1` sees the certificate and re-enables
`ui_access` automatically.

Know what this does before running it: the certificate's public half goes
into `LocalMachine\Root` and `LocalMachine\TrustedPublisher`, so anything
signed with it is trusted on that machine. The private key stays in
`Cert:\CurrentUser\My`. Remove all of it with:

```powershell
.\resources\scripts\dev-cert.ps1 -Remove
```

`-DevSign` refuses to run when `$env:CI` is set, so it cannot reach a
release build.

## Release builds

A self-signed certificate is useless to anyone else — their machine does
not trust it, so they get no UIAccess and a SmartScreen warning. Shipping
needs a real certificate.

`SignFiles` in `package.ps1` already drives `azuresigntool`. Setting these
six variables is the only work required; no code changes:

`AZ_VAULT_URL`, `AZ_CERT_NAME`, `AZ_CLIENT_ID`, `AZ_CLIENT_SECRET`,
`AZ_TENANT_ID`, `RFC3161_TIMESTAMP_URL`

`.github/workflows/package.yaml` already passes them through as secrets.

### Choosing a certificate

**Azure Artifact Signing** (renamed from Trusted Signing) is the
recommended option: $9.99/month for 5,000 signatures, no hardware token,
and it works with the existing `azuresigntool` path. Generally available
in the US, Canada and Europe since January 2026.

One constraint: **individual** developers must be in the US or Canada for
public-trust certificates. Organisations have a wider list.

If neither applies, a traditional OV certificate is the fallback. Certum
is the cheap route at roughly €100/year, but the private key ships on a
physical hardware token, which does not automate well in CI.

Do not pay for EV. Since 2024 it no longer skips SmartScreen reputation
building, so it costs more and buys nothing here.
