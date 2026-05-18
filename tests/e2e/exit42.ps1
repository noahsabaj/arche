$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent (Split-Path -Parent $scriptDir)
$outputPath = Join-Path $repoRoot "build\e2e\exit42"
$outputDir = Split-Path -Parent $outputPath

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

if (!(Get-Command wsl -ErrorAction SilentlyContinue)) {
    Write-Error "wsl.exe is required to run the generated Linux ELF"
    exit 1
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue

Push-Location $repoRoot
try {
    & cargo run --manifest-path ".\bootstrap\archec0\Cargo.toml" -- ".\examples\exit42.arc" "-o" ".\build\e2e\exit42"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "archec0 failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    $wslPath = ConvertTo-WslPath -Path $outputPath
    & wsl $wslPath
    Assert-ExitCode -Actual $LASTEXITCODE -Expected 42

    Write-Host "PASS: exit42 exits 42"
}
finally {
    Pop-Location
}
