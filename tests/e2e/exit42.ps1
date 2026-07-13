param(
    [switch] $SkipGeneratedLinuxExecution
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent (Split-Path -Parent $scriptDir)
$outputPath = Join-Path $repoRoot "build/e2e/exit42"
$outputDir = Split-Path -Parent $outputPath
$isWindowsPlatform = [System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT
$isLinuxPlatform = $false
if (!$isWindowsPlatform) {
    $isLinuxVariable = Get-Variable -Name IsLinux -ErrorAction SilentlyContinue
    $isLinuxPlatform = $null -ne $isLinuxVariable -and [bool] $isLinuxVariable.Value
}

function ConvertTo-WslPath {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $resolved = (Resolve-Path -LiteralPath $Path).Path

    if ($resolved -notmatch '^([A-Za-z]):\\(.*)$') {
        Write-Error "Cannot convert non-drive path to WSL path: $resolved"
        exit 1
    }

    $drive = $matches[1].ToLowerInvariant()
    $rest = $matches[2] -replace '\\', '/'
    return "/mnt/$drive/$rest"
}

function Assert-ExitCode {
    param(
        [Parameter(Mandatory = $true)]
        [int] $Actual,

        [Parameter(Mandatory = $true)]
        [int] $Expected
    )

    if ($Actual -ne $Expected) {
        Write-Error "Expected exit code $Expected but got $Actual"
        exit 1
    }
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue

Push-Location $repoRoot
try {
    & cargo run --locked --manifest-path "./bootstrap/archec0/Cargo.toml" -- "./examples/exit42.arc" "-o" "./build/e2e/exit42"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "archec0 failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    if ($SkipGeneratedLinuxExecution) {
        Write-Host "SKIP: exit42 generated Linux execution (-SkipGeneratedLinuxExecution)"
        return
    }

    if ($isWindowsPlatform) {
        if (!(Get-Command wsl -ErrorAction SilentlyContinue)) {
            Write-Error "wsl.exe is required to run the generated Linux ELF; use -SkipGeneratedLinuxExecution to skip explicitly"
            exit 1
        }

        $wslPath = ConvertTo-WslPath -Path $outputPath
        & wsl $wslPath
    } elseif ($isLinuxPlatform) {
        $resolvedOutputPath = (Resolve-Path -LiteralPath $outputPath).Path
        & $resolvedOutputPath
    } else {
        Write-Error "Generated Linux ELF execution is supported only on Windows through WSL or directly on Linux; use -SkipGeneratedLinuxExecution to skip explicitly"
        exit 1
    }

    Assert-ExitCode -Actual $LASTEXITCODE -Expected 42

    Write-Host "PASS: exit42 exits 42"
}
finally {
    Pop-Location
}
