param(
    [switch] $SkipGeneratedLinuxExecution,
    [string] $CompilerPath = "",
    [string] $BuildDirectory = ""
)

. (Join-Path $PSScriptRoot "_proof_helpers.ps1")

if ($BuildDirectory.Length -eq 0) {
    $BuildDirectory = Join-Path $script:E2eRepoRoot "build/e2e/arithmetic"
}
if (Test-Path -LiteralPath $BuildDirectory) {
    Remove-Item -LiteralPath $BuildDirectory -Recurse -Force
}
[IO.Directory]::CreateDirectory($BuildDirectory) | Out-Null

try {
    $compiler = Get-E2eCompiler $CompilerPath
    foreach ($fixture in @("math", "sub42", "mul42")) {
        $source = Join-Path $script:E2eRepoRoot "examples/$fixture.arc"
        $artifact = Join-Path $BuildDirectory $fixture
        $compile = Invoke-E2eProcess "compile arithmetic fixture $fixture" `
            $compiler @($source, "-o", $artifact)
        Assert-E2eStatus $compile 0
        Assert-E2eV2Artifact $artifact

        $run = Invoke-E2eArtifact "run arithmetic fixture $fixture" `
            $artifact -SkipGeneratedLinuxExecution:$SkipGeneratedLinuxExecution
        if ($null -ne $run) {
            Assert-E2eStatus $run 42
            Assert-E2e ($run.Stderr.Length -eq 0) "$fixture wrote unexpected stderr"
            $normalized = $run.Stdout.Replace("`r`n", "`n").Replace("`r", "`n")
            Assert-E2e ($normalized -eq "ARCHEOBS2`nEND`n") `
                "$fixture observation is not the canonical empty-world stream"
            Assert-E2eObservation $fixture $run.Stdout
        }
    }
    Write-Host "PASS: arithmetic e2e uses semantic status, not instruction shape"
}
finally {
    if (Test-Path -LiteralPath $BuildDirectory) {
        Remove-Item -LiteralPath $BuildDirectory -Recurse -Force
    }
}
