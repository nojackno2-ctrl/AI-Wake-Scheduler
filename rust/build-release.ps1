<#
.SYNOPSIS
  建置 Rust 版的 AI 倒數喚醒，並把執行檔複製成 dist\rust\AI倒數喚醒.exe。

.DESCRIPTION
  先執行測試（可用 -SkipTests 略過），再以 release 設定建置。
  產物是單一執行檔，不需要另外安裝執行階段。
#>
param(
    [switch]$SkipTests,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\dist\rust')
)

$ErrorActionPreference = 'Stop'

# cargo 把進度寫到 stderr；在 Windows PowerShell 5.1 下，Stop 會把被重導的 stderr 當成錯誤，
# 所以執行 cargo 時暫時放寬，改以結束碼判斷成敗。
function Invoke-Cargo {
    param([string]$What, [string[]]$Arguments)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & cargo @Arguments
        $code = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previous
    }
    if ($code -ne 0) { throw "$What失敗（結束碼 $code）。" }
}

Push-Location $PSScriptRoot
try {
    if (-not $SkipTests) {
        Invoke-Cargo '測試' @('test', '--workspace', '--locked')
    }
    Invoke-Cargo '建置' @('build', '--release', '--locked', '-p', 'aiwake-app')

    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
    $target = Join-Path $OutputDirectory 'AI倒數喚醒.exe'
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'target\release\ai-wake-scheduler.exe') -Destination $target -Force
    $hash = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
    Write-Host "已產生 $target"
    Write-Host "SHA256 $hash"
}
finally {
    Pop-Location
}
