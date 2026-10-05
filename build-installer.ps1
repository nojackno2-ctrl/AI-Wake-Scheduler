# =====================================================================
# AI 倒數喚醒 (AI Wake Scheduler) - Windows 安裝包一鍵自動建置腳本
# =====================================================================

[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [switch]$SkipTests = $false,
    [switch]$RunIntegrationTests = $false,
    [string]$OutputDirectory = ""
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir

$projectFile = Join-Path $ScriptDir "src\AiWakeScheduler.WinForms\AiWakeScheduler.WinForms.csproj"
$installerScript = (Get-ChildItem -Path (Join-Path $ScriptDir "installer") -Filter "*.iss" | Select-Object -First 1).FullName
[xml]$projectXml = Get-Content -Raw -LiteralPath $projectFile
$projectVersion = [string]$projectXml.Project.PropertyGroup.Version
$installerVersionMatch = [regex]::Match(
    (Get-Content -Raw -LiteralPath $installerScript),
    '(?m)#define\s+MyAppVersion\s+"([^"]+)"')
if ([string]::IsNullOrWhiteSpace($projectVersion) -or
    -not $installerVersionMatch.Success -or
    $installerVersionMatch.Groups[1].Value -ne $projectVersion) {
    throw "Version mismatch: csproj=$projectVersion, installer=$($installerVersionMatch.Groups[1].Value)."
}

Write-Host "======================================================" -ForegroundColor Cyan
Write-Host "  AI Wake Scheduler - Building Windows Setup.exe" -ForegroundColor Cyan
Write-Host "======================================================" -ForegroundColor Cyan

# 1. Locate Inno Setup compiler
$isccPath = $null
$candidates = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Antigravity IDE\resources\app\node_modules\innosetup\bin\ISCC.exe"
)

foreach ($cand in $candidates) {
    if (Test-Path $cand) {
        $isccPath = $cand
        break
    }
}

if (-not $isccPath) {
    $cmd = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if ($cmd) {
        $isccPath = $cmd.Source
    }
}

if (-not $isccPath) {
    Write-Error "Inno Setup 6 compiler (ISCC.exe) not found."
    exit 1
}

Write-Host "[1/5] Inno Setup compiler found: $isccPath" -ForegroundColor Green

# 2. Run tests
if (-not $SkipTests) {
    Write-Host "[2/5] Running deterministic tests..." -ForegroundColor Yellow
    & dotnet run --project tests/AiWakeScheduler.Tests/AiWakeScheduler.Tests.csproj -c $Configuration
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Unit tests failed, build aborted."
        exit $LASTEXITCODE
    }
    Write-Host "      Deterministic tests passed!" -ForegroundColor Green
    $uiTestDir = Join-Path $ScriptDir "bin\verification-tests"
    & dotnet publish tests/AiWakeScheduler.WinForms.Tests/AiWakeScheduler.WinForms.Tests.csproj -c $Configuration -r win-x64 --self-contained true -o $uiTestDir
    if ($LASTEXITCODE -ne 0) { throw "WinForms test publish failed." }
    & (Join-Path $uiTestDir "AiWakeScheduler.WinForms.Tests.exe")
    if ($LASTEXITCODE -ne 0) { throw "WinForms tests failed." }
    if ($RunIntegrationTests) {
        Write-Host "      Running integration tests..." -ForegroundColor Yellow
        & dotnet run --project tests/AiWakeScheduler.Tests/AiWakeScheduler.Tests.csproj -c $Configuration -- --integration
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Integration tests failed, build aborted."
            exit $LASTEXITCODE
        }
    }
} else {
    Write-Host "[2/5] Skipping tests (SkipTests is active)" -ForegroundColor DarkGray
}

# 3. Publish Self-Contained Win-x64
$publishDir = Join-Path $ScriptDir "bin\publish-selfcontained"

Write-Host "[3/5] Publishing Self-Contained Win-x64 binaries..." -ForegroundColor Yellow
& dotnet publish src/AiWakeScheduler.WinForms/AiWakeScheduler.WinForms.csproj `
    -c $Configuration `
    -r win-x64 `
    --self-contained true `
    -p:PublishSingleFile=false `
    -o $publishDir

if ($LASTEXITCODE -ne 0) {
    Write-Error "dotnet publish failed, build aborted."
    exit $LASTEXITCODE
}
Write-Host "      Publish completed: $publishDir" -ForegroundColor Green

$icoSource = Join-Path $ScriptDir "assets\app.ico"
$icoDest = Join-Path $publishDir "app.ico"
if ((Test-Path $icoSource) -and (-not (Test-Path $icoDest))) {
    Copy-Item $icoSource $icoDest -Force
}

# 4. Preserve existing installer outputs
$distDir = if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { Join-Path $ScriptDir "dist" } else { [IO.Path]::GetFullPath($OutputDirectory) }
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
}
$outputName = "AI倒數喚醒_Setup_v${projectVersion}_x64_$(Get-Date -Format 'yyyyMMdd-HHmmss')"

# 5. Run Inno Setup Compiler
$issScript = $installerScript
Write-Host "[4/5] Compiling installer with Inno Setup..." -ForegroundColor Yellow

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $isccPath
$psi.Arguments = "/Q /O`"$distDir`" /F$outputName `"$issScript`""
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true
$p = [System.Diagnostics.Process]::Start($psi)
$p.WaitForExit()
$exitCode = $p.ExitCode
$p.Dispose()

if ($exitCode -ne 0) {
    Write-Error "Inno Setup compilation failed with exit code: $exitCode"
    exit $exitCode
}

# 6. Complete and generate checksums
$installer = Get-Item -LiteralPath (Join-Path $distDir "$outputName.exe") -ErrorAction SilentlyContinue

if ($installer) {
    $hash = Get-FileHash -Path $installer.FullName -Algorithm SHA256
    $sizeMb = [Math]::Round($installer.Length / 1MB, 2)
    Set-Content -LiteralPath (Join-Path $distDir "$outputName.sha256") -Value "$($hash.Hash)  $($installer.Name)" -Encoding utf8
    Write-Host ""
    Write-Host "======================================================" -ForegroundColor Cyan
    Write-Host "  Build completed successfully!" -ForegroundColor Green
    Write-Host "  Installer: $($installer.Name)" -ForegroundColor White
    Write-Host "  Size     : $sizeMb MB ($($installer.Length) bytes)" -ForegroundColor White
    Write-Host "  Path     : $($installer.FullName)" -ForegroundColor White
    Write-Host "  SHA256   : $($hash.Hash)" -ForegroundColor DarkGray
    Write-Host "======================================================" -ForegroundColor Cyan
} else {
    Write-Error "Installer binary not found in dist!"
    exit 1
}
