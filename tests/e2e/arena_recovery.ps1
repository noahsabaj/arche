param(
    [switch] $SkipGeneratedLinuxExecution,
    [string] $CompilerPath = "",
    [string] $BuildDirectory = ""
)

. (Join-Path $PSScriptRoot "_proof_helpers.ps1")

if ($BuildDirectory.Length -eq 0) {
    $BuildDirectory = Join-Path $script:E2eRepoRoot "build/e2e/arena-recovery"
}
if (Test-Path -LiteralPath $BuildDirectory) {
    Remove-Item -LiteralPath $BuildDirectory -Recurse -Force
}
[IO.Directory]::CreateDirectory($BuildDirectory) | Out-Null

try {
    $compiler = Get-E2eCompiler $CompilerPath
    $source = Join-Path $script:E2eRepoRoot "examples/arena_recovery.arc"
    $artifact = Join-Path $BuildDirectory "arena-recovery"

    $check = Invoke-E2eProcess "Arena executable check" $compiler @($source, "--check")
    Assert-E2eStatus $check 0
    $core1 = Invoke-E2eProcess "Arena Core emission 1" $compiler @($source, "--emit-core")
    $core2 = Invoke-E2eProcess "Arena Core emission 2" $compiler @($source, "--emit-core")
    Assert-E2eStatus $core1 0
    Assert-E2eStatus $core2 0
    Assert-E2e ($core1.Stdout -eq $core2.Stdout) "Arena Core emission is nondeterministic"
    Assert-E2e ($core1.Stdout.Contains("world Arena")) "Arena Core omitted its world identity"
    Assert-E2e ($core1.Stdout.Contains("system Recover")) "Arena Core omitted Recover"

    $compile = Invoke-E2eProcess "compile structurally distinct Arena" `
        $compiler @($source, "-o", $artifact)
    Assert-E2eStatus $compile 0
    Assert-E2eV2Artifact $artifact

    $first = Invoke-E2eArtifact "run Arena 1" $artifact `
        -SkipGeneratedLinuxExecution:$SkipGeneratedLinuxExecution
    if ($null -ne $first) {
        Assert-E2eStatus $first 0
        Assert-E2e ($first.Stderr.Length -eq 0) "Arena wrote unexpected stderr"
        Assert-E2eObservation "Arena" $first.Stdout
        Assert-E2e (($first.Stdout -split "`n" | Where-Object { $_ -match '^ROW ' }).Count -eq 5) `
            "Arena observation did not contain five committed rows"

        $second = Invoke-E2eArtifact "run Arena 2" $artifact `
            -SkipGeneratedLinuxExecution:$SkipGeneratedLinuxExecution
        Assert-E2eStatus $second 0
        Assert-E2e ($second.Stdout -eq $first.Stdout) `
            "Arena observation changed across ASLR executions"
        Assert-E2e ($second.Stderr -eq $first.Stderr) `
            "Arena stderr changed across ASLR executions"
    }
    Write-Host "PASS: Arena recovery e2e"
}
finally {
    if (Test-Path -LiteralPath $BuildDirectory) {
        Remove-Item -LiteralPath $BuildDirectory -Recurse -Force
    }
}
