<#
.SYNOPSIS
    Signs one file with the Authenticode certificate Tauri's bundler selected.
.DESCRIPTION
    Tauri invokes the command configured as bundle.windows.signCommand once for
    the application executable and once for each installer it produces, with the
    path substituted for %1. Signing at that point is what makes the executable
    inside the installer signed as well: signing the installers afterwards would
    only cover the wrapper.

    The certificate must already be in the current user's store, carrying the
    private key. The release workflow imports it there first, from a secret, on
    the otherwise-ephemeral runner. A thumbprint alone is not a credential: a
    runner that has not had the certificate imported cannot sign, and pretending
    otherwise is how a release ships unsigned.

    Trusted timestamping is always requested so the signature outlives the
    certificate.
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Path,

    [string]$CertificateThumbprint = $env:SIGN_CERTIFICATE_THUMBPRINT,
    [string]$TimestampUrl = $env:SIGN_TIMESTAMP_URL,
    [string]$DigestAlgorithm = 'SHA256',

    # A production release must not silently produce an unsigned artifact.
    [switch]$RequireSigning,

    # An explicit opt-out for developers reproducing the bundling step locally.
    [switch]$AllowUnsigned
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Path)) {
    throw "nothing to sign at '$Path'"
}

if ($TimestampUrl -eq '') {
    # RFC 3161 over HTTPS, so the signature remains valid after the signing
    # certificate expires.
    $TimestampUrl = 'http://timestamp.digicert.com'
}

if (-not $CertificateThumbprint) {
    if ($RequireSigning) {
        throw 'signing was required but no certificate thumbprint was supplied: set SIGN_CERTIFICATE_THUMBPRINT'
    }
    if ($AllowUnsigned) {
        Write-Host "signing skipped for $Path because no certificate is configured"
        exit 0
    }
    Write-Warning "no signing certificate is configured; $Path will be unsigned"
    exit 0
}

$certificate = Get-Item -Path "Cert:\CurrentUser\My\$CertificateThumbprint" -ErrorAction SilentlyContinue
if (-not $certificate) {
    throw "the signing certificate $CertificateThumbprint is not in the current user's store. A thumbprint is not a credential: the certificate has to be imported onto this machine first."
}
if (-not $certificate.HasPrivateKey) {
    throw "the certificate $CertificateThumbprint has no private key, so it cannot sign"
}

Write-Host "signing $Path"
$signature = Set-AuthenticodeSignature -FilePath $Path -Certificate $certificate -TimestampServer $TimestampUrl -HashAlgorithm $DigestAlgorithm

if ($signature.Status -ne 'Valid') {
    throw "signing $Path failed with status $($signature.Status): $($signature.StatusMessage)"
}

Write-Host "signed $Path"
exit 0
