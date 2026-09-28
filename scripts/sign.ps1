<#
.SYNOPSIS
  Code-sign MCPanel release artifacts. Pluggable: the signing method is chosen by
  environment variables so the release pipeline does not change when the certificate
  type changes (decision #12).

.DESCRIPTION
  MCPANEL_SIGN_MODE:
    (unset)      -> nothing is signed; the artifact is reported as UNSIGNED.
    certstore    -> signtool with a certificate in the Windows certificate store,
                    selected by MCPANEL_SIGN_CERT_THUMBPRINT.
  Other modes (for example Azure Trusted Signing) are added here when chosen.

  Writes `signed=true|false` to $env:GITHUB_OUTPUT when running in GitHub Actions.
  Never prints secrets.
#>
param([Parameter(Mandatory = $true)][string[]]$Path)
$ErrorActionPreference = "Stop"

function Set-Output([string]$Value) {
  if ($env:GITHUB_OUTPUT) { "signed=$Value" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8 }
}

$mode = $env:MCPANEL_SIGN_MODE
if ([string]::IsNullOrWhiteSpace($mode)) {
  Write-Warning "No signing configured (MCPANEL_SIGN_MODE unset). Artifacts are UNSIGNED and must not be published as a stable release."
  Set-Output "false"
  exit 0
}

switch ($mode) {
  "certstore" {
    if (-not $env:MCPANEL_SIGN_CERT_THUMBPRINT) { throw "MCPANEL_SIGN_CERT_THUMBPRINT is required for certstore signing" }
    $ts = if ($env:MCPANEL_SIGN_TIMESTAMP_URL) { $env:MCPANEL_SIGN_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $signtool) { throw "signtool.exe not found" }
    foreach ($p in $Path) {
      & $signtool.FullName sign /fd SHA256 /sha1 $env:MCPANEL_SIGN_CERT_THUMBPRINT /tr $ts /td SHA256 $p
      if ($LASTEXITCODE -ne 0) { throw "signing failed for $p" }
      & $signtool.FullName verify /pa $p
      if ($LASTEXITCODE -ne 0) { throw "signature verification failed for $p" }
    }
    Set-Output "true"
  }
  default { throw "Unknown MCPANEL_SIGN_MODE '$mode'" }
}
