<#
.SYNOPSIS
    Signs one file with the Authenticode certificate Tauri's bundler chose.
.DESCRIPTION
    Tauri invokes the command configured as bundle.windows.signCommand once for
    the application executable and once for each installer it produces, with the
    path substituted for %1. Signing at that point is what makes the executable
    inside the installer signed as well: signing the installers afterwards would
    only cover the wrapper.

    The certificate is supplied either as a thumbprint (already imported into the
    certificate store by the release workflow) or as a PKCS#12 file plus its
    password. Trusted timestamping is always requested so the signature outlives
    the certificate.
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Path,

    [string]$CertificateThumbprint = $env:SIGN_CERTIFICATE_THUMBPRINT,
    [string]$CertificateFile = $env:SIGN_CERTIFICATE_FILE,
    [string]$CertificatePassword = $env:SIGN_CERTIFICATE_PASSWORD,
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
    # RFC 3161 over HTTPS; required so the signature remains valid after the
    # signing certificate expires.
    $TimestampUrl = 'http://timestamp.digicert.com'
}

$certificate = $null
if ($CertificateThumbprint) {
    $certificate = Get-Item -Path "Cert:\CurrentUser\My\$CertificateThumbprint" -ErrorAction SilentlyContinue
    if (-not $certificate) {
        throw "the signing certificate $CertificateThumbprint is not in the current user's store"
    }
} elseif ($CertificateFile) {
    if ($CertificatePassword) {
        $secure = ConvertTo-SecureString -String $CertificatePassword -Force -AsPlainText
        $certificate = Get-PfxCertificate -FilePath $CertificateFile -Password $secure
    } else {
        $certificate = Get-PfxCertificate -FilePath $CertificateFile
    }
}

if (-not $certificate) {
    if ($RequireSigning) {
        throw 'signing was required but no certificate was supplied: set SIGN_CERTIFICATE_THUMBPRINT or SIGN_CERTIFICATE_FILE'
    }
    if ($AllowUnsigned) {
        Write-Host "signing skipped for $Path because no certificate is configured"
        exit 0
    }
    Write-Warning "no signing certificate is configured; $Path will be unsigned"
    exit 0
}

Write-Host "signing $Path"
$result = Set-AuthenticodeSignature `
    -FilePath $Path `
    -Certificate $certificate `
    -TimestampServer $TimestampUrl `
    -HashAlgorithm $DigestAlgorithm

if ($result.Status -ne 'Valid') {
    throw "signing $Path failed with status $($result.Status): $($result.StatusMessage)"
}

Write-Host "signed $Path"
exit 0
