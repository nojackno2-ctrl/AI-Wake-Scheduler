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
$installerScript = Join-Path $ScriptDir "installer\AI倒數喚醒.iss"
[xml]$projectXml = Get-Content -Raw -LiteralPath $projectFile
$projectVersion = [string]$projectXml.Project.PropertyGroup.Version
$installerVersionMatch = [regex]::Match(
    (Get-Content -Raw -LiteralPath $installerScript),
    '(?m)^#define MyAppVersion "([^"]+)"$')
if ($projectVersion -ne $expectedVersion -or
    -not $installerVersionMatch.Success -or
    $installerVersionMatch.Groups[1].Value -ne $expectedVersion) {
    throw "版本不一致：預期 $expectedVersion，csproj=$projectVersion，installer=$($installerVersionMatch.Groups[1].Value)。"
}

Write-Host "======================================================" -ForegroundColor Cyan
Write-Host "  AI 倒數喚醒 - 開始建置 Windows 安裝版 (Setup.exe)" -ForegroundColor Cyan
Write-Host "======================================================" -ForegroundColor Cyan

# 0. 關閉任何可能鎖定檔案的舊有執行程式
Get-Process | Where-Object { $_.ProcessName -match "^(AI倒數喚醒(_Setup_)?.*|ISCC)$" } | Stop-Process -Force -ErrorAction SilentlyContinue

# 1. 尋找 Inno Setup 編譯器 (ISCC.exe)
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
    Write-Error "找不到 Inno Setup 6 編譯器 (ISCC.exe)。請先安裝 Inno Setup 6 或將 ISCC.exe 加入 PATH。"
    exit 1
}

Write-Host "[1/5] Inno Setup 編譯器定位成功: $isccPath" -ForegroundColor Green

# 2. 執行單元測試 (確保品質)
if (-not $SkipTests) {
    Write-Host "[2/5] 執行可重現的 deterministic 測試..." -ForegroundColor Yellow
    & dotnet run --project tests/AiWakeScheduler.Tests/AiWakeScheduler.Tests.csproj -c $Configuration
    if ($LASTEXITCODE -ne 0) {
        Write-Error "單元測試執行失敗，建置中止。"
        exit $LASTEXITCODE
    }
    Write-Host "      deterministic 測試全數通過！" -ForegroundColor Green
    if ($RunIntegrationTests) {
        Write-Host "      執行 opt-in 本機 CLI／登入整合測試..." -ForegroundColor Yellow
        & dotnet run --project tests/AiWakeScheduler.Tests/AiWakeScheduler.Tests.csproj -c $Configuration -- --integration
        if ($LASTEXITCODE -ne 0) {
            Write-Error "本機 CLI／登入整合測試失敗，建置中止。"
            exit $LASTEXITCODE
        }
    }
} else {
    Write-Host "[2/5] 跳過單元測試 (SkipTests 已啟用)" -ForegroundColor DarkGray
}

# 3. 發布 Self-Contained Win-x64 程式檔案
$publishDir = Join-Path $ScriptDir "bin\publish-selfcontained"
if (Test-Path $publishDir) {
    try {
        Remove-Item $publishDir -Recurse -Force -ErrorAction SilentlyContinue
    } catch {
        # 若有暫時鎖定則由 dotnet publish 直接覆寫
    }
}

Write-Host "[3/5] 正在發布 Self-Contained Win-x64 獨立執行檔..." -ForegroundColor Yellow
& dotnet publish src/AiWakeScheduler.WinForms/AiWakeScheduler.WinForms.csproj `
    -c $Configuration `
    -r win-x64 `
    --self-contained true `
    -p:PublishSingleFile=false `
    -o $publishDir

if ($LASTEXITCODE -ne 0) {
    Write-Error "dotnet publish 發布失敗，建置中止。"
    exit $LASTEXITCODE
}
Write-Host "      發布成功: $publishDir" -ForegroundColor Green

# 確保圖示已複製至發布目錄
$icoSource = Join-Path $ScriptDir "assets\app.ico"
$icoDest = Join-Path $publishDir "app.ico"
if ((Test-Path $icoSource) -and (-not (Test-Path $icoDest))) {
    Copy-Item $icoSource $icoDest -Force
}

# 4. 確保輸出目錄存在並清理舊安裝檔
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

# 5. 調用 Inno Setup 編譯安裝檔
$issScript = $installerScript
Write-Host "[4/5] 正在使用 Inno Setup 編譯安裝程式..." -ForegroundColor Yellow

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $isccPath
$psi.Arguments = "/Q `"$issScript`""
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true
$p = [System.Diagnostics.Process]::Start($psi)
$p.WaitForExit()
$exitCode = $p.ExitCode
$p.Dispose()

# 確保編譯器結束後立即釋放所有控制代碼
Get-Process -Name "ISCC" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

if ($exitCode -ne 0) {
    Write-Error "Inno Setup 編譯失敗，結束代碼: $exitCode"
    exit $exitCode
}

# 6. 完成並輸出資訊
$installer = Get-ChildItem -Path $distDir -Filter "AI倒數喚醒_Setup_*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1

if ($installer) {
    $hash = Get-FileHash -Path $installer.FullName -Algorithm SHA256
    $sizeMb = [Math]::Round($installer.Length / 1MB, 2)
    Write-Host ""
    Write-Host "======================================================" -ForegroundColor Cyan
    Write-Host "  建置完成！安裝程式已產出至：" -ForegroundColor Green
    Write-Host "  檔案名稱: $($installer.Name)" -ForegroundColor White
    Write-Host "  檔案大小: $sizeMb MB ($($installer.Length) 位元組)" -ForegroundColor White
    Write-Host "  完整路徑: $($installer.FullName)" -ForegroundColor White
    Write-Host "  SHA256  : $($hash.Hash)" -ForegroundColor DarkGray
    Write-Host "======================================================" -ForegroundColor Cyan
} else {
    Write-Error "找不到產出的安裝檔！"
    exit 1
}
