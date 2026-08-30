param(
    [switch] $SkipGeneratedLinuxExecution,
    [string] $CompilerPath = "",
    [string] $BuildDirectory = ""
)

. (Join-Path $PSScriptRoot "_proof_helpers.ps1")

if ($BuildDirectory.Length -eq 0) {
    $BuildDirectory = Join-Path $script:E2eRepoRoot "build/e2e/exit42"
}
if (Test-Path -LiteralPath $BuildDirectory) {
    Remove-Item -LiteralPath $BuildDirectory -Recurse -Force
}
[IO.Directory]::CreateDirectory($BuildDirectory) | Out-Null

try {
    $compiler = Get-E2eCompiler $CompilerPath
    $source = Join-Path $script:E2eRepoRoot "examples/exit42.arc"
    $artifact = Join-Path $BuildDirectory "exit42"
    $compile = Invoke-E2eProcess "compile exit42" $compiler @($source, "-o", $artifact)
    Assert-E2eStatus $compile 0
    Assert-E2eV2Artifact $artifact

    $run = Invoke-E2eArtifact "run exit42" $artifact `
        -SkipGeneratedLinuxExecution:$SkipGeneratedLinuxExecution
    if ($null -ne $run) {
        Assert-E2eStatus $run 42
        Assert-E2e ($run.Stderr.Length -eq 0) "exit42 wrote unexpected stderr"
        $normalized = $run.Stdout.Replace("`r`n", "`n").Replace("`r", "`n")
        Assert-E2e ($normalized -eq "ARCHEOBS2`nEND`n") `
            "exit42 observation is not the canonical empty-world stream"
        Assert-E2eObservation "exit42" $run.Stdout
    }
    Write-Host "PASS: exit42 e2e"
}
finally {
    if (Test-Path -LiteralPath $BuildDirectory) {
        Remove-Item -LiteralPath $BuildDirectory -Recurse -Force
    }
}
