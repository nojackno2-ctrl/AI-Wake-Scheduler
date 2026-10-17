<#
.SYNOPSIS
    Visual Studio NMake 專案用的 cargo 包裝腳本。

.DESCRIPTION
    執行 cargo 並把診斷訊息轉成 Visual Studio「錯誤清單」可辨識的
    `檔案(行,欄): error 代碼: 訊息` 格式，點兩下即可跳到原始碼。

    Build  ：cargo build（Release 設定加 --release）
    Test   ：cargo fmt --check、cargo clippy -D warnings、cargo test
    Clean  ：cargo clean
#>
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Build', 'Test', 'Clean')]
    [string]$Action,

    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Debug'
)

$ErrorActionPreference = 'Stop'
# cargo 與 rustc 以 UTF-8 輸出；讓繁體中文訊息在輸出視窗正確顯示。
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$env:CARGO_TERM_COLOR = 'never'

$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root

$diagnostic = '^(?<file>(?:[A-Za-z]:)?[^:]+):(?<line>\d+):(?<col>\d+): (?<sev>error|warning)(?:\[(?<code>[^\]]+)\])?: (?<msg>.*)$'
$panic = "^thread '(?<test>[^']+)'(?: \(\d+\))? panicked at (?<file>(?:[A-Za-z]:)?[^:]+):(?<line>\d+):(?<col>\d+):?$"
$rustfmt = '^Diff in (?:\\\\\?\\)?(?<file>.+?\.rs)(?::(?<line>\d+)| at line (?<line>\d+))?:$'

function Resolve-SourcePath([string]$file) {
    if ([System.IO.Path]::IsPathRooted($file)) { return $file }
    return Join-Path $root $file
}

function Invoke-Cargo([string]$arguments) {
    Write-Host "> cargo $arguments"
    # 透過 cmd 合併 stderr，避免 Windows PowerShell 5.1 把原生 stderr 包成錯誤紀錄。
    & cmd.exe /d /c "cargo $arguments 2>&1" | ForEach-Object {
        $line = [string]$_
        if ($line -match $diagnostic) {
            $code = if ($Matches.code) { $Matches.code } else { 'rustc' }
            "{0}({1},{2}): {3} {4}: {5}" -f (Resolve-SourcePath $Matches.file), $Matches.line, $Matches.col, $Matches.sev, $code, $Matches.msg
        }
        elseif ($line -match $panic) {
            "{0}({1},{2}): error test: 測試 {3} 失敗" -f (Resolve-SourcePath $Matches.file), $Matches.line, $Matches.col, $Matches.test
            $line
        }
        elseif ($line -match $rustfmt) {
            $at = if ($Matches.line) { $Matches.line } else { '1' }
            "{0}({1}): error rustfmt: 格式不符，請執行 cargo fmt --all" -f (Resolve-SourcePath $Matches.file), $at
        }
        else {
            $line
        }
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Host "cargo $arguments 失敗（exit $LASTEXITCODE）"
        exit $LASTEXITCODE
    }
}

switch ($Action) {
    'Build' {
        $release = if ($Configuration -eq 'Release') { ' --release' } else { '' }
        Invoke-Cargo "build --workspace --locked --message-format=short$release"
    }
    'Test' {
        Invoke-Cargo 'fmt --all -- --check'
        Invoke-Cargo 'clippy --workspace --all-targets --locked --message-format=short -- -D warnings'
        Invoke-Cargo 'test --workspace --locked --message-format=short'
    }
    'Clean' {
        Invoke-Cargo 'clean'
    }
}
exit 0
