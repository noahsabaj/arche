Set-StrictMode -Version 2.0
$ErrorActionPreference = "Stop"

if ($PSVersionTable.PSEdition -ne "Core" -or $PSVersionTable.PSVersion.Major -lt 7 -or ($PSVersionTable.PSVersion.Major -eq 7 -and $PSVersionTable.PSVersion.Minor -lt 6)) {
    Write-Error "Arche e2e proof helpers require PowerShell Core 7.6.5 or higher (found $($PSVersionTable.PSEdition) $($PSVersionTable.PSVersion)). Please run with 'pwsh'."
    exit 1
}

$script:E2eRepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script:E2eIsWindows = $IsWindows
$script:E2eIsLinux = $IsLinux

function Assert-E2e {
    param([bool] $Condition, [string] $Message)
    if (!$Condition) {
        throw $Message
    }
}

function ConvertTo-E2eArgument {
    param([AllowEmptyString()][string] $Value)
    if ($Value.Length -eq 0) {
        return '""'
    }
    if ($Value -notmatch '[\s"]') {
        return $Value
    }
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Invoke-E2eProcess {
    param(
        [string] $Name,
        [string] $Executable,
        [string[]] $Arguments = @()
    )

    Write-Host "==> $Name"
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $Executable
    $startInfo.Arguments = (($Arguments | ForEach-Object {
        ConvertTo-E2eArgument $_
    }) -join " ")
    $startInfo.WorkingDirectory = $script:E2eRepoRoot
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    try {
        Assert-E2e $process.Start() "$Name could not start"
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $stdout = $stdoutTask.Result
        $stderr = $stderrTask.Result
        $status = $process.ExitCode
    }
    finally {
        $process.Dispose()
    }
    return [PSCustomObject]@{
        Name = $Name
        Status = $status
        Stdout = $stdout
        Stderr = $stderr
    }
}

function Assert-E2eStatus {
    param($Result, [int] $Expected)
    if ($Result.Status -ne $Expected) {
        if ($Result.Stdout.Length -ne 0) { Write-Host "stdout:`n$($Result.Stdout)" }
        if ($Result.Stderr.Length -ne 0) { Write-Host "stderr:`n$($Result.Stderr)" }
        throw "$($Result.Name) expected status $Expected but got $($Result.Status)"
    }
    Write-Host "PASS: $($Result.Name) (status $Expected)"
}

function Get-E2eCompiler {
    param([AllowEmptyString()][string] $CompilerPath)
    if ($CompilerPath.Length -ne 0) {
        Assert-E2e (Test-Path -LiteralPath $CompilerPath -PathType Leaf) `
            "compiler does not exist: $CompilerPath"
        return (Resolve-Path -LiteralPath $CompilerPath).Path
    }

    $manifest = Join-Path $script:E2eRepoRoot "bootstrap/archec0/Cargo.toml"
    $build = Invoke-E2eProcess "build archec0 for e2e" "cargo" `
        @("build", "--locked", "--manifest-path", $manifest)
    Assert-E2eStatus $build 0
    $name = if ($script:E2eIsWindows) { "archec0.exe" } else { "archec0" }
    return Join-Path $script:E2eRepoRoot "bootstrap/archec0/target/debug/$name"
}

function ConvertTo-E2eWslPath {
    param([string] $Path)
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    Assert-E2e ($resolved -match '^([A-Za-z]):\\(.*)$') `
        "cannot translate path to WSL: $resolved"
    return "/mnt/$($matches[1].ToLowerInvariant())/$($matches[2].Replace('\', '/'))"
}

function Invoke-E2eArtifact {
    param(
        [string] $Name,
        [string] $Path,
        [switch] $SkipGeneratedLinuxExecution
    )

    if ($SkipGeneratedLinuxExecution) {
        Write-Host "SKIP: $Name generated Linux execution was explicitly disabled"
        return $null
    }
    if ($script:E2eIsLinux) {
        return Invoke-E2eProcess $Name (Resolve-Path -LiteralPath $Path).Path
    }
    if ($script:E2eIsWindows) {
        Assert-E2e ($null -ne (Get-Command wsl.exe -ErrorAction SilentlyContinue)) `
            "WSL is required unless -SkipGeneratedLinuxExecution is supplied"
        return Invoke-E2eProcess $Name "wsl.exe" @((ConvertTo-E2eWslPath $Path))
    }
    throw "generated Linux execution requires Linux or WSL"
}

function Assert-E2eV2Artifact {
    param([string] $Path)
    Assert-E2e (Test-Path -LiteralPath $Path -PathType Leaf) "artifact was not published: $Path"
    [byte[]]$bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path).Path)
    Assert-E2e ($bytes.Length -ge 344) "artifact is too short for segmented ELF headers"
    Assert-E2e (
        $bytes[0] -eq 0x7f -and
        [Text.Encoding]::ASCII.GetString($bytes, 1, 3) -eq "ELF"
    ) "artifact does not have ELF magic"
    Assert-E2e ([BitConverter]::ToUInt16($bytes, 16) -eq 3) "artifact is not ET_DYN"
    Assert-E2e ([BitConverter]::ToUInt16($bytes, 56) -eq 5) `
        "artifact does not have the four PT_LOAD plus GNU-stack layout"

    [UInt64]$programHeaders = [BitConverter]::ToUInt64($bytes, 32)
    [UInt64]$programHeaderSize = [BitConverter]::ToUInt16($bytes, 54)
    $metadata = $null
    for ($index = 0; $index -lt 5; $index++) {
        [int]$row = [int]($programHeaders + [UInt64]$index * $programHeaderSize)
        [UInt32]$kind = [BitConverter]::ToUInt32($bytes, $row)
        [UInt32]$flags = [BitConverter]::ToUInt32($bytes, $row + 4)
        [UInt64]$offset = [BitConverter]::ToUInt64($bytes, $row + 8)
        if ($kind -eq 1 -and $flags -eq 4 -and $offset -ne 0) {
            Assert-E2e ($null -eq $metadata) "artifact has multiple metadata segments"
            $metadata = [PSCustomObject]@{
                Offset = $offset
                Length = [BitConverter]::ToUInt64($bytes, $row + 32)
            }
        }
        Assert-E2e (($flags -band 3) -ne 3) "artifact contains a writable executable segment"
    }
    Assert-E2e ($null -ne $metadata) "artifact has no R-- metadata segment"
    Assert-E2e ($metadata.Offset + $metadata.Length -eq [UInt64]$bytes.Length) `
        "metadata segment is not trailing"
    Assert-E2e ([Text.Encoding]::ASCII.GetString($bytes, [int]$metadata.Offset, 8) -eq "ARCHEECS") `
        "metadata segment does not contain ARCHEECS"
    Assert-E2e ([BitConverter]::ToUInt32($bytes, [int]$metadata.Offset + 8) -eq 2) `
        "metadata is not ARCHEECS v2"
    Assert-E2e ([BitConverter]::ToUInt32($bytes, [int]$metadata.Offset + 12) -eq 64) `
        "metadata header is not 64 bytes"
    Assert-E2e ([BitConverter]::ToUInt64($bytes, [int]$metadata.Offset + 40) -eq 14) `
        "metadata does not contain all 14 canonical sections"
    Assert-E2e ([BitConverter]::ToUInt64($bytes, [int]$metadata.Offset + 48) -eq 64) `
        "metadata directory rows are not 64 bytes"
    Write-Host "PASS: segmented ARCHEECS v2 artifact"
}

function Assert-E2eObservation {
    param([string] $Name, [string] $Text)
    $normalized = $Text.Replace("`r`n", "`n").Replace("`r", "`n")
    Assert-E2e ($normalized.StartsWith("ARCHEOBS2`n")) "$Name omitted ARCHEOBS2 header"
    Assert-E2e ($normalized.EndsWith("END`n")) "$Name omitted canonical END record"
    $lines = @($normalized.Substring(0, $normalized.Length - 1).Split([char]10))
    for ($lineIndex = 1; $lineIndex -lt $lines.Count - 1; $lineIndex++) {
        $line = $lines[$lineIndex]
        $valid =
            $line -match '^RESOURCE [0-9A-F]{32} UNINITIALIZED$' -or
            $line -match '^RESOURCE [0-9A-F]{32} INITIALIZED [0-9]+ (-|[0-9A-F]+)$' -or
            $line -match '^TABLE [0-9]+(?: [0-9A-F]{32})* [0-9]+$' -or
            $line -match '^ROW [0-9]+ [0-9]+ [0-9]+$' -or
            $line -match '^COLUMN [0-9A-F]{32} [0-9]+ (-|[0-9A-F]+)$'
        Assert-E2e $valid "$Name emitted invalid ARCHEOBS2 record: $line"
    }
    Write-Host "PASS: ARCHEOBS2 grammar for $Name"
}
