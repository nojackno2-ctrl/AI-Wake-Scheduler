# =====================================================================
# AI 倒數喚醒 (AI Wake Scheduler) - Windows 安裝包一鍵自動建置腳本 (Pure Rust)
# =====================================================================

[CmdletBinding()]
param(
    [switch]$SkipTests = $false,
    [switch]$RunIntegrationTests = $false,
    [switch]$VerificationBuild = $false,
    [string]$OutputDirectory = ""
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir

$cargoToml = Join-Path $ScriptDir "Cargo.toml"
$installerScript = (Get-ChildItem -Path (Join-Path $ScriptDir "installer") -Filter "*.iss" | Select-Object -First 1).FullName
$cargoContent = Get-Content -Raw -LiteralPath $cargoToml
$versionMatch = [regex]::Match($cargoContent, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) {
    throw "Cannot parse version from Cargo.toml."
}
$projectVersion = $versionMatch.Groups[1].Value
$installerVersionMatch = [regex]::Match(
    (Get-Content -Raw -LiteralPath $installerScript),
    '(?m)#define\s+MyAppVersion\s+"([^"]+)"')
if ([string]::IsNullOrWhiteSpace($projectVersion) -or
    -not $installerVersionMatch.Success -or
    $installerVersionMatch.Groups[1].Value -ne $projectVersion) {
    throw "Version mismatch: Cargo.toml=$projectVersion, installer=$($installerVersionMatch.Groups[1].Value)."
}

Write-Host "======================================================" -ForegroundColor Cyan
Write-Host "  AI Wake Scheduler - Building Windows Setup.exe (Rust)" -ForegroundColor Cyan
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
    Write-Host "[2/5] Running deterministic Rust tests..." -ForegroundColor Yellow
    & cargo test --workspace --locked
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Cargo tests failed, build aborted."
        exit $LASTEXITCODE
    }
    Write-Host "      Deterministic tests passed!" -ForegroundColor Green
    if ($RunIntegrationTests) {
        Write-Host "      Running integration tests..." -ForegroundColor Yellow
        & cargo test -p aiwake-core --locked -- --ignored executable_locator_resolution
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Integration tests failed, build aborted."
            exit $LASTEXITCODE
        }
    }
} else {
    Write-Host "[2/5] Skipping tests (SkipTests is active)" -ForegroundColor DarkGray
}

# 3. Build release Rust binary
$publishDir = Join-Path $ScriptDir "bin\publish-release"

Write-Host "[3/5] Building release Rust binary..." -ForegroundColor Yellow
& cargo build --release --locked -p aiwake-app

if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build failed, build aborted."
    exit $LASTEXITCODE
}

if (Test-Path $publishDir) {
    Remove-Item -Recurse -Force $publishDir
}
New-Item -ItemType Directory -Path $publishDir -Force | Out-Null
$exeSource = Join-Path $ScriptDir "target\release\ai-wake-scheduler.exe"
$exeDest = Join-Path $publishDir "AI倒數喚醒.exe"
Copy-Item $exeSource $exeDest -Force
Write-Host "      Release binary ready: $exeDest" -ForegroundColor Green

# 4. Preserve existing installer outputs
$distDir = if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { Join-Path $ScriptDir "dist" } else { [IO.Path]::GetFullPath($OutputDirectory) }
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
}
$outputPrefix = if ($VerificationBuild) { "AI倒數喚醒_QA_Setup" } else { "AI倒數喚醒_Setup" }
$outputName = "${outputPrefix}_v${projectVersion}_x64_$(Get-Date -Format 'yyyyMMdd-HHmmss')"

# 5. Run Inno Setup Compiler
$issScript = $installerScript
Write-Host "[4/5] Compiling installer with Inno Setup..." -ForegroundColor Yellow

$isccArgs = @(
    "/Q",
    "/O$distDir",
    "/F$outputName",
    "/DPublishDir=$publishDir"
)
if ($VerificationBuild) {
    $isccArgs += "/DVerificationBuild"
}
$isccArgs += $issScript

& $isccPath @isccArgs
$exitCode = $LASTEXITCODE
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
