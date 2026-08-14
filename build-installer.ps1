# =====================================================================
# AI 倒數喚醒 (AI Wake Scheduler) - Windows 安裝包一鍵自動建置腳本
# =====================================================================

[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [switch]$SkipTests = $false,
    [switch]$RunIntegrationTests = $false
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir

$expectedVersion = "1.2.0"
$projectFile = Join-Path $ScriptDir "src\AiWakeScheduler.WinForms\AiWakeScheduler.WinForms.csproj"
$installerScript = (Get-ChildItem -Path (Join-Path $ScriptDir "installer") -Filter "*.iss" | Select-Object -First 1).FullName
[xml]$projectXml = Get-Content -Raw -LiteralPath $projectFile
$projectVersion = [string]$projectXml.Project.PropertyGroup.Version
$installerVersionMatch = [regex]::Match(
    (Get-Content -Raw -LiteralPath $installerScript),
    '(?m)#define\s+MyAppVersion\s+"([^"]+)"')
if ($projectVersion -ne $expectedVersion -or
    -not $installerVersionMatch.Success -or
    $installerVersionMatch.Groups[1].Value -ne $expectedVersion) {
    throw "Version mismatch: Expected $expectedVersion, csproj=$projectVersion, installer=$($installerVersionMatch.Groups[1].Value)."
}

Write-Host "======================================================" -ForegroundColor Cyan
Write-Host "  AI Wake Scheduler - Building Windows Setup.exe" -ForegroundColor Cyan
Write-Host "======================================================" -ForegroundColor Cyan

# 0. Terminate old running instances
Get-Process | Where-Object { $_.ProcessName -match "^(AI倒數喚醒(_Setup_)?.*|ISCC)$" } | Stop-Process -Force -ErrorAction SilentlyContinue

# 1. Locate Inno Setup compiler
$isccPath = $null
$candidates = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
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
if (Test-Path $publishDir) {
    try {
        Remove-Item $publishDir -Recurse -Force -ErrorAction SilentlyContinue
    } catch {
    }
}

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

# 4. Clean old installer outputs
$distDir = Join-Path $ScriptDir "dist"
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
} else {
    $existing = Get-ChildItem -Path $distDir -Filter "AI倒數喚醒_Setup_*.exe" -ErrorAction SilentlyContinue
    foreach ($f in $existing) {
        for ($i = 0; $i -lt 5; $i++) {
            try {
                Remove-Item $f.FullName -Force -ErrorAction Stop
                break
            } catch {
                Start-Sleep -Milliseconds 500
            }
        }
    }
}

# 5. Run Inno Setup Compiler
$issScript = $installerScript
Write-Host "[4/5] Compiling installer with Inno Setup..." -ForegroundColor Yellow

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $isccPath
$psi.Arguments = "/Q `"$issScript`""
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true
$p = [System.Diagnostics.Process]::Start($psi)
$p.WaitForExit()
$exitCode = $p.ExitCode
$p.Dispose()

Get-Process -Name "ISCC" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

if ($exitCode -ne 0) {
    Write-Error "Inno Setup compilation failed with exit code: $exitCode"
    exit $exitCode
}

# 6. Complete and generate checksums
$installer = Get-ChildItem -Path $distDir -Filter "*_Setup_*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1

if ($installer) {
    $hash = Get-FileHash -Path $installer.FullName -Algorithm SHA256
    $sizeMb = [Math]::Round($installer.Length / 1MB, 2)
    Set-Content -LiteralPath (Join-Path $distDir "SHA256SUMS.txt") -Value "$($hash.Hash)  $($installer.Name)" -Encoding utf8
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
