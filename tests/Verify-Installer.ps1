[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SetupPath,
    [ValidateSet('Install', 'UpgradeKeepStartup', 'UpgradeEnableStartup', 'UpgradeDisableStartup', 'Uninstall')]
    [string]$Phase = 'Install'
)

$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$qaRoot = Join-Path $repoRoot 'bin\verification-installed'
$qaEvidence = Join-Path $repoRoot 'bin\verification-lifecycle'
$setup = [IO.Path]::GetFullPath($SetupPath)
$qaRunName = 'AI倒數喚醒-QA'
$qaUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{59A78022-7C7F-4CEE-B6B2-9B6D5588A57B}_is1'
$qaGroup = 'AI 倒數喚醒 QA'
$qaMenu = Join-Path ([Environment]::GetFolderPath('Programs')) $qaGroup

function Assert-QA([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

# This script must never run a normal installer or reuse a user's installation.
Assert-QA (Test-Path -LiteralPath $setup) 'QA installer does not exist.'
Assert-QA ([Diagnostics.FileVersionInfo]::GetVersionInfo($setup).ProductName.Trim() -eq 'AI 倒數喚醒 QA') 'Only VerificationBuild installers are accepted.'
Assert-QA ($qaRoot.StartsWith((Join-Path $repoRoot 'bin\'), [StringComparison]::OrdinalIgnoreCase)) 'QA installation must remain inside workspace/bin.'
New-Item -ItemType Directory -Path $qaEvidence -Force | Out-Null

function Get-OriginalState {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Run')
    try {
        $state = [ordered]@{ SettingsHash = $null; SchedulesHash = $null; RunValue = $key.GetValue('AI倒數喚醒') }
        foreach ($item in @(@('settings.json', 'SettingsHash'), @('schedules.json', 'SchedulesHash'))) {
            $path = Join-Path "$env:LOCALAPPDATA\AI倒數喚醒" $item[0]
            if (Test-Path -LiteralPath $path) { $state[$item[1]] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
        }
        return $state
    } finally { $key.Dispose() }
}

$baselinePath = Join-Path $qaEvidence 'original-state.json'
if ($Phase -eq 'Install') {
    Assert-QA (-not (Test-Path -LiteralPath $qaRoot)) 'Fresh QA installation directory already exists.'
    Assert-QA (-not (Test-Path -LiteralPath $qaMenu)) 'QA shortcut group already exists.'
    Get-OriginalState | ConvertTo-Json | Set-Content -LiteralPath $baselinePath
}
Assert-QA (Test-Path -LiteralPath $baselinePath) 'Original-state baseline is missing.'

$start = [Diagnostics.ProcessStartInfo]::new()
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.FileName = if ($Phase -eq 'Uninstall') { Join-Path $qaRoot 'unins000.exe' } else { $setup }
Assert-QA (Test-Path -LiteralPath $start.FileName) 'Phase executable is missing.'
foreach ($argument in @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=$(Join-Path $qaEvidence "$Phase.log")")) {
    $start.ArgumentList.Add($argument)
}
if ($Phase -ne 'Uninstall') {
    foreach ($argument in @('/CURRENTUSER', "/DIR=$qaRoot", "/GROUP=$qaGroup")) { $start.ArgumentList.Add($argument) }
    if ($Phase -in @('Install', 'UpgradeEnableStartup')) { $start.ArgumentList.Add('/TASKS=startupicon') }
    if ($Phase -eq 'UpgradeDisableStartup') { $start.ArgumentList.Add('/TASKS=!startupicon,!desktopicon') }
}
$process = [Diagnostics.Process]::Start($start)
$process.WaitForExit()
Assert-QA ($process.ExitCode -eq 0) "Installer phase failed with exit code $($process.ExitCode)."
$process.Dispose()

$runKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Run')
$uninstallKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($qaUninstallKey)
try {
    $runValue = $runKey.GetValue($qaRunName)
    if ($Phase -eq 'Uninstall') {
        Assert-QA ($null -eq $uninstallKey) 'QA uninstall registration remains.'
        Assert-QA ($null -eq $runValue) 'QA startup entry remains after uninstall.'
        Assert-QA (-not (Test-Path -LiteralPath (Join-Path $qaRoot 'AI倒數喚醒.exe'))) 'QA executable remains after uninstall.'
        Assert-QA (-not (Test-Path -LiteralPath $qaMenu)) 'QA shortcut group remains after uninstall.'
    } else {
        Assert-QA ($null -ne $uninstallKey) 'QA uninstall registration is missing.'
        Assert-QA (Test-Path -LiteralPath (Join-Path $qaRoot 'AI倒數喚醒.exe')) 'QA executable is missing.'
        Assert-QA (Test-Path -LiteralPath (Join-Path $qaMenu 'AI 倒數喚醒 QA.lnk')) 'QA launch shortcut is missing.'
        Assert-QA ((Get-ChildItem -LiteralPath $qaMenu -Filter '*.lnk').Count -eq 2) 'Expected launch and uninstall shortcuts.'
        if ($Phase -eq 'UpgradeDisableStartup') {
            Assert-QA ($null -eq $runValue) 'Explicitly disabling startup during upgrade did not remove the entry.'
        } else {
            Assert-QA ($runValue -eq "`"$(Join-Path $qaRoot 'AI倒數喚醒.exe')`" --minimized --verify-ui") 'QA startup entry is missing or malformed.'
        }
    }
} finally {
    $runKey.Dispose()
    if ($null -ne $uninstallKey) { $uninstallKey.Dispose() }
}
$baseline = Get-Content -LiteralPath $baselinePath -Raw | ConvertFrom-Json
$current = Get-OriginalState
foreach ($property in @('SettingsHash', 'SchedulesHash', 'RunValue')) {
    Assert-QA ($current[$property] -eq $baseline.$property) "Formal application state changed: $property"
}
"PASS installer $Phase; formal settings, schedules and startup unchanged."
