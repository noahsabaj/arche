param(
    [switch] $SkipGeneratedLinuxExecution
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptDir
$manifestPath = Join-Path $repoRoot "bootstrap/archec0/Cargo.toml"
$proofRoot = Join-Path $repoRoot "build/m26-proof"
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$isWindowsPlatform = [System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT
$isLinuxPlatform = $false
if (!$isWindowsPlatform) {
    $isLinuxVariable = Get-Variable -Name IsLinux -ErrorAction SilentlyContinue
    $isLinuxPlatform = $null -ne $isLinuxVariable -and [bool]$isLinuxVariable.Value
}

function Assert-True {
    param(
        [Parameter(Mandatory = $true)]
        [bool] $Condition,

        [Parameter(Mandatory = $true)]
        [string] $Message
    )

    if (!$Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $false)]
        $Actual,

        [Parameter(Mandatory = $false)]
        $Expected
    )

    if ($Actual -ne $Expected) {
        throw "$Name expected '$Expected' but got '$Actual'"
    }
}

function Assert-Contains {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string] $Actual,

        [Parameter(Mandatory = $true)]
        [string] $Expected
    )

    if (!$Actual.Contains($Expected)) {
        throw "$Name expected text '$Expected'"
    }
}

function Normalize-LineEndings {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string] $Text
    )

    return $Text.Replace("`r`n", "`n").Replace("`r", "`n")
}

function ConvertTo-ProcessArgument {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string] $Value
    )

    if ($Value.Length -eq 0) {
        return '""'
    }
    if ($Value -notmatch '[\s"]') {
        return $Value
    }

    # All proof paths are controlled by this repository. Quoting whitespace and
    # embedded quotes is sufficient for ProcessStartInfo on .NET Framework 4.8
    # and current .NET, which keeps the runner compatible with PowerShell 5.1.
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Invoke-CapturedProcess {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string] $Executable,

        [Parameter(Mandatory = $false)]
        [string[]] $Arguments = @(),

        [Parameter(Mandatory = $false)]
        [string] $WorkingDirectory = $repoRoot
    )

    Write-Host "==> $Name"
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $Executable
    $startInfo.Arguments = (($Arguments | ForEach-Object {
        ConvertTo-ProcessArgument -Value $_
    }) -join " ")
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    try {
        Assert-True -Condition $process.Start() -Message "$Name could not start"
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

function Copy-ProofDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Source,

        [Parameter(Mandatory = $true)]
        [string] $Destination
    )

    [System.IO.Directory]::CreateDirectory($Destination) | Out-Null
    foreach ($entry in @(Get-ChildItem -LiteralPath $Source -Force)) {
        Copy-Item -LiteralPath $entry.FullName -Destination $Destination -Recurse -Force
    }
}

function Get-RelativeProofFiles {
    param([Parameter(Mandatory = $true)][string] $Root)

    $prefixLength = $Root.TrimEnd([IO.Path]::DirectorySeparatorChar).Length + 1
    return @(Get-ChildItem -LiteralPath $Root -Recurse -File -Force |
        ForEach-Object { $_.FullName.Substring($prefixLength) } |
        Sort-Object)
}

function Assert-NoLockTemporaries {
    param([Parameter(Mandatory = $true)][string] $Root)

    $temporaries = @(Get-RelativeProofFiles -Root $Root | Where-Object {
        [IO.Path]::GetFileName($_).Contains(".Arche.lock.arche-tmp-")
    })
    Assert-Equal -Name "M27-B sibling lock temporary cleanup" `
        -Actual $temporaries.Count -Expected 0
}

function Test-M27BPublicCheck {
    param([Parameter(Mandatory = $true)][string] $PublicCli)

    $fixtureRoot = Join-Path $repoRoot "tests/m27b"
    $project = Join-Path $proofRoot "m27b-public-check"
    Copy-ProofDirectory -Source (Join-Path $fixtureRoot "mixed-workspace") `
        -Destination $project
    $nested = Join-Path $project "nested/working-directory"
    [System.IO.Directory]::CreateDirectory($nested) | Out-Null
    $before = @(Get-RelativeProofFiles -Root $project)

    $discovered = Invoke-CapturedProcess -Name "M27-B nested manifest discovery" `
        -Executable $PublicCli -Arguments @("check") -WorkingDirectory $nested
    Assert-ProcessStatus $discovered 0
    Assert-Equal -Name "M27-B check stderr" -Actual $discovered.Stderr -Expected ""
    Assert-Equal -Name "M27-B stable resolution summary" `
        -Actual (Normalize-LineEndings $discovered.Stdout) `
        -Expected "arche: resolved packages=1 targets=3 modules=5`n"

    $after = @(Get-RelativeProofFiles -Root $project)
    $created = @($after | Where-Object { $before -notcontains $_ })
    Assert-Equal -Name "M27-B check-only publication count" `
        -Actual $created.Count -Expected 1
    Assert-Equal -Name "M27-B check-only publication" `
        -Actual $created[0] -Expected "Arche.lock"
    $lockPath = Join-Path $project "Arche.lock"
    $lockBytes = [System.IO.File]::ReadAllBytes($lockPath)
    Assert-True -Condition ($lockBytes.Length -gt 0) `
        -Message "M27-B canonical lock is empty"
    Assert-True -Condition ($lockBytes -notcontains [byte] 13) `
        -Message "M27-B canonical lock contains CR bytes"

    [System.IO.File]::WriteAllText(
        $lockPath,
        "incomplete lock bytes`n",
        $utf8NoBom
    )

    $explicit = Invoke-CapturedProcess -Name "M27-B explicit manifest path" `
        -Executable $PublicCli `
        -Arguments @("check", "--manifest-path", (Join-Path $project "Arche.toml"))
    Assert-ProcessStatus $explicit 0
    Assert-Equal -Name "M27-B explicit manifest stderr" `
        -Actual $explicit.Stderr -Expected ""
    Assert-Equal -Name "M27-B discovery and explicit summaries" `
        -Actual $explicit.Stdout -Expected $discovered.Stdout
    $lockBase64 = [Convert]::ToBase64String($lockBytes)
    $repeatedLockBase64 = [Convert]::ToBase64String(
        [System.IO.File]::ReadAllBytes($lockPath)
    )
    Assert-Equal -Name "M27-B repeated canonical lock" `
        -Actual $repeatedLockBase64 -Expected $lockBase64
    Assert-Equal -Name "M27-B repeated check file inventory" `
        -Actual ((Get-RelativeProofFiles -Root $project) -join "`n") `
        -Expected ($after -join "`n")
    Assert-NoLockTemporaries -Root $project

    $pathWorkspace = Join-Path $proofRoot "m27b-path-workspace"
    Copy-ProofDirectory -Source (Join-Path $fixtureRoot "path-workspace") `
        -Destination $pathWorkspace
    $pathWorkspaceNested = Join-Path $pathWorkspace "packages/shared/src"
    $pathWorkspaceBefore = @(Get-RelativeProofFiles -Root $pathWorkspace)
    $pathResolution = Invoke-CapturedProcess `
        -Name "M27-B multi-member path dependency" `
        -Executable $PublicCli -Arguments @("check") `
        -WorkingDirectory $pathWorkspaceNested
    Assert-ProcessStatus $pathResolution 0
    Assert-Equal -Name "M27-B multi-member stderr" `
        -Actual $pathResolution.Stderr -Expected ""
    Assert-Equal -Name "M27-B multi-member summary" `
        -Actual (Normalize-LineEndings $pathResolution.Stdout) `
        -Expected "arche: resolved packages=2 targets=2 modules=2`n"
    $pathWorkspaceAfter = @(Get-RelativeProofFiles -Root $pathWorkspace)
    $pathWorkspaceCreated = @($pathWorkspaceAfter | Where-Object {
        $pathWorkspaceBefore -notcontains $_
    })
    Assert-Equal -Name "M27-B multi-member publication count" `
        -Actual $pathWorkspaceCreated.Count -Expected 1
    Assert-Equal -Name "M27-B multi-member publication" `
        -Actual $pathWorkspaceCreated[0] -Expected "Arche.lock"
    $pathLockPath = Join-Path $pathWorkspace "Arche.lock"
    $pathLock = [System.IO.File]::ReadAllText($pathLockPath)
    Assert-Contains -Name "M27-B workspace authority lock row" `
        -Actual (Normalize-LineEndings $pathLock) `
        -Expected "[workspace]`nsource-digest = `"sha256:"
    Assert-Contains -Name "M27-B multi-member app lock row" `
        -Actual $pathLock -Expected 'name = "example/app"'
    Assert-Contains -Name "M27-B multi-member dependency lock row" `
        -Actual $pathLock -Expected 'name = "example/shared"'
    Assert-Contains -Name "M27-B path dependency lock edge" `
        -Actual $pathLock `
        -Expected 'alias = "shared", package = "example/shared"'

    $pathManifestPath = Join-Path $pathWorkspace "Arche.toml"
    $pathManifest = [System.IO.File]::ReadAllText($pathManifestPath)
    $changedPathManifest = $pathManifest.Replace(
        'default-members = ["packages/app", "packages/shared"]',
        'default-members = ["packages/app"]'
    )
    Assert-True -Condition ($changedPathManifest -ne $pathManifest) `
        -Message "M27-B virtual workspace fixture defaults were not found"
    [System.IO.File]::WriteAllText(
        $pathManifestPath,
        $changedPathManifest,
        $utf8NoBom
    )
    $changedDefaults = Invoke-CapturedProcess `
        -Name "M27-B virtual workspace authority lock replacement" `
        -Executable $PublicCli -Arguments @("check") `
        -WorkingDirectory $pathWorkspaceNested
    Assert-ProcessStatus $changedDefaults 0
    Assert-Equal -Name "M27-B changed defaults stderr" `
        -Actual $changedDefaults.Stderr -Expected ""
    Assert-Equal -Name "M27-B changed defaults summary" `
        -Actual $changedDefaults.Stdout -Expected $pathResolution.Stdout
    $changedPathLock = [System.IO.File]::ReadAllText($pathLockPath)
    Assert-True -Condition ($changedPathLock -ne $pathLock) `
        -Message "M27-B virtual workspace authority change did not replace the lock"
    Assert-Equal -Name "M27-B changed defaults file inventory" `
        -Actual ((Get-RelativeProofFiles -Root $pathWorkspace) -join "`n") `
        -Expected ($pathWorkspaceAfter -join "`n")
    Assert-NoLockTemporaries -Root $pathWorkspace

    $legacy = Join-Path $proofRoot "m27b-legacy-startup"
    Copy-ProofDirectory -Source (Join-Path $fixtureRoot "legacy-startup") `
        -Destination $legacy
    $legacyLockPath = Join-Path $legacy "Arche.lock"
    $legacyLockBytes = $utf8NoBom.GetBytes("previous complete lock`n")
    [System.IO.File]::WriteAllBytes($legacyLockPath, $legacyLockBytes)
    $legacyBefore = @(Get-RelativeProofFiles -Root $legacy)
    $migration = Invoke-CapturedProcess -Name "M27-B startup hard cut" `
        -Executable $PublicCli -Arguments @("check") -WorkingDirectory $legacy
    Assert-ProcessStatus $migration 1
    Assert-Equal -Name "M27-B migration stdout" -Actual $migration.Stdout -Expected ""
    Assert-Contains -Name "M27-B migration diagnostic code" `
        -Actual $migration.Stderr -Expected "error[MIGRATE001]"
    Assert-Contains -Name "M27-B migration world header" `
        -Actual $migration.Stderr -Expected "M26 ``world Name`` headers"
    Assert-Contains -Name "M27-B migration entrypoint" `
        -Actual $migration.Stderr -Expected "``fn main``"
    Assert-Equal -Name "M27-B failed check file inventory" `
        -Actual ((Get-RelativeProofFiles -Root $legacy) -join "`n") `
        -Expected ($legacyBefore -join "`n")
    Assert-Equal -Name "M27-B failed check preserves complete lock" `
        -Actual ([Convert]::ToBase64String(
            [System.IO.File]::ReadAllBytes($legacyLockPath)
        )) `
        -Expected ([Convert]::ToBase64String($legacyLockBytes))
    Assert-NoLockTemporaries -Root $legacy

    $registry = Join-Path $proofRoot "m27b-registry-unavailable"
    Copy-ProofDirectory -Source (Join-Path $fixtureRoot "registry-unavailable") `
        -Destination $registry
    $registryBefore = @(Get-RelativeProofFiles -Root $registry)
    $unavailable = Invoke-CapturedProcess -Name "M27-B unavailable registry source" `
        -Executable $PublicCli -Arguments @("check") -WorkingDirectory $registry
    Assert-ProcessStatus $unavailable 1
    Assert-Equal -Name "M27-B unavailable registry stdout" `
        -Actual $unavailable.Stdout -Expected ""
    Assert-Contains -Name "M27-B unavailable registry diagnostic code" `
        -Actual $unavailable.Stderr -Expected "error[DEPENDENCY001]"
    Assert-Contains -Name "M27-B unavailable registry package" `
        -Actual $unavailable.Stderr -Expected "example/remote"
    Assert-Equal -Name "M27-B unavailable registry file inventory" `
        -Actual ((Get-RelativeProofFiles -Root $registry) -join "`n") `
        -Expected ($registryBefore -join "`n")

    $toolchainMismatch = Join-Path $proofRoot "m27b-toolchain-mismatch"
    Copy-ProofDirectory -Source (Join-Path $fixtureRoot "toolchain-mismatch") `
        -Destination $toolchainMismatch
    $toolchainLockPath = Join-Path $toolchainMismatch "Arche.lock"
    $toolchainLockBytes = $utf8NoBom.GetBytes("previous complete lock`n")
    [System.IO.File]::WriteAllBytes($toolchainLockPath, $toolchainLockBytes)
    $toolchainBefore = @(Get-RelativeProofFiles -Root $toolchainMismatch)
    $toolchain = Invoke-CapturedProcess `
        -Name "M27-B incompatible toolchain requirement" `
        -Executable $PublicCli -Arguments @("check") `
        -WorkingDirectory $toolchainMismatch
    Assert-ProcessStatus $toolchain 2
    Assert-Equal -Name "M27-B incompatible toolchain stdout" `
        -Actual $toolchain.Stdout -Expected ""
    Assert-Contains -Name "M27-B incompatible toolchain code" `
        -Actual $toolchain.Stderr -Expected "error[MANIFEST004]"
    Assert-Contains -Name "M27-B incompatible toolchain requirement" `
        -Actual $toolchain.Stderr `
        -Expected "package ``example/future`` requires Arche ``>=1.0.0``, but selected toolchain is ``0.0.0``"
    Assert-Equal -Name "M27-B toolchain rejection file inventory" `
        -Actual ((Get-RelativeProofFiles -Root $toolchainMismatch) -join "`n") `
        -Expected ($toolchainBefore -join "`n")
    Assert-Equal -Name "M27-B toolchain rejection preserves complete lock" `
        -Actual ([Convert]::ToBase64String(
            [System.IO.File]::ReadAllBytes($toolchainLockPath)
        )) `
        -Expected ([Convert]::ToBase64String($toolchainLockBytes))
    Assert-NoLockTemporaries -Root $toolchainMismatch

    $malformedManifest = Join-Path $proofRoot "m27b-malformed-manifest"
    Copy-ProofDirectory -Source (Join-Path $fixtureRoot "malformed-manifest") `
        -Destination $malformedManifest
    $malformedBefore = @(Get-RelativeProofFiles -Root $malformedManifest)
    $malformed = Invoke-CapturedProcess -Name "M27-B malformed manifest" `
        -Executable $PublicCli -Arguments @("check") `
        -WorkingDirectory $malformedManifest
    Assert-ProcessStatus $malformed 2
    Assert-Equal -Name "M27-B malformed manifest stdout" `
        -Actual $malformed.Stdout -Expected ""
    Assert-Contains -Name "M27-B malformed manifest code" `
        -Actual $malformed.Stderr -Expected "error[MANIFEST002]"
    Assert-Contains -Name "M27-B malformed manifest schema" `
        -Actual $malformed.Stderr `
        -Expected "unsupported Arche.toml schema 2; expected schema 1"
    Assert-Equal -Name "M27-B malformed manifest file inventory" `
        -Actual ((Get-RelativeProofFiles -Root $malformedManifest) -join "`n") `
        -Expected ($malformedBefore -join "`n")

    $invalid = Invoke-CapturedProcess -Name "M27-B malformed check arguments" `
        -Executable $PublicCli -Arguments @("check", "--manifest-path")
    Assert-ProcessStatus $invalid 2
    Assert-Equal -Name "M27-B malformed check stdout" `
        -Actual $invalid.Stdout -Expected ""
    Assert-Equal -Name "M27-B malformed check diagnostic" `
        -Actual (Normalize-LineEndings $invalid.Stderr) `
        -Expected "arche: invalid arguments for ``check```nusage: arche check [--manifest-path <Arche.toml>]`n"
    Write-Host "PASS: M27-B public project, workspace, lock, and migration contracts"
}

function Assert-ProcessStatus {
    param(
        [Parameter(Mandatory = $true)]
        $Result,

        [Parameter(Mandatory = $true)]
        [int] $Expected
    )

    if ($Result.Status -ne $Expected) {
        if ($Result.Stdout.Length -ne 0) {
            Write-Host "stdout:`n$($Result.Stdout)"
        }
        if ($Result.Stderr.Length -ne 0) {
            Write-Host "stderr:`n$($Result.Stderr)"
        }
        throw "$($Result.Name) expected status $Expected but got $($Result.Status)"
    }
    Write-Host "PASS: $($Result.Name) (status $Expected)"
}

function Invoke-Compiler {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Compiler,

        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $false)]
        [string[]] $Arguments = @()
    )

    return Invoke-CapturedProcess -Name $Name -Executable $Compiler -Arguments $Arguments
}

function Write-Utf8File {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [string] $Text
    )

    [System.IO.File]::WriteAllText($Path, $Text, $utf8NoBom)
}

function Read-U16 {
    param([byte[]] $Bytes, [UInt64] $Offset)
    return [BitConverter]::ToUInt16($Bytes, [int]$Offset)
}

function Read-U32 {
    param([byte[]] $Bytes, [UInt64] $Offset)
    return [BitConverter]::ToUInt32($Bytes, [int]$Offset)
}

function Read-U64 {
    param([byte[]] $Bytes, [UInt64] $Offset)
    return [BitConverter]::ToUInt64($Bytes, [int]$Offset)
}

function Write-U32 {
    param([byte[]] $Bytes, [UInt64] $Offset, [UInt32] $Value)
    $encoded = [BitConverter]::GetBytes($Value)
    for ($index = 0; $index -lt $encoded.Length; $index++) {
        $Bytes[[int]$Offset + $index] = $encoded[$index]
    }
}

function Write-U64 {
    param([byte[]] $Bytes, [UInt64] $Offset, [UInt64] $Value)
    $encoded = [BitConverter]::GetBytes($Value)
    for ($index = 0; $index -lt $encoded.Length; $index++) {
        $Bytes[[int]$Offset + $index] = $encoded[$index]
    }
}

function Align-Up {
    param([UInt64] $Value, [UInt64] $Alignment)
    return [UInt64](($Value + $Alignment - 1) -band (-bnot ($Alignment - 1)))
}

function Test-PowerOfTwo {
    param([UInt64] $Value)
    return $Value -ne 0 -and (($Value -band ($Value - 1)) -eq 0)
}

function Get-StaticPieLayout {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    Assert-True -Condition (Test-Path -LiteralPath $Path -PathType Leaf) `
        -Message "ELF artifact does not exist: $Path"
    $resolvedPath = (Resolve-Path -LiteralPath $Path).Path
    [byte[]]$bytes = [System.IO.File]::ReadAllBytes($resolvedPath)
    Assert-True -Condition ($bytes.Length -ge 344) `
        -Message "ELF artifact is too short for five program headers"
    Assert-Equal -Name "ELF magic" `
        -Actual ([Text.Encoding]::ASCII.GetString($bytes, 1, 3)) -Expected "ELF"
    Assert-Equal -Name "ELF magic prefix" -Actual $bytes[0] -Expected 0x7f
    Assert-Equal -Name "ELF class" -Actual $bytes[4] -Expected 2
    Assert-Equal -Name "ELF endianness" -Actual $bytes[5] -Expected 1
    Assert-Equal -Name "ELF type" -Actual (Read-U16 $bytes 16) -Expected 3
    Assert-Equal -Name "ELF machine" -Actual (Read-U16 $bytes 18) -Expected 0x3e
    Assert-Equal -Name "ELF header size" -Actual (Read-U16 $bytes 52) -Expected 64
    Assert-Equal -Name "ELF program-header size" -Actual (Read-U16 $bytes 54) -Expected 56
    Assert-Equal -Name "ELF program-header count" -Actual (Read-U16 $bytes 56) -Expected 5
    Assert-Equal -Name "ELF section-header offset" -Actual (Read-U64 $bytes 40) -Expected 0
    Assert-Equal -Name "ELF section-header count" -Actual (Read-U16 $bytes 60) -Expected 0

    [UInt64]$programHeaderOffset = Read-U64 $bytes 32
    [UInt64]$programHeaderSize = Read-U16 $bytes 54
    [UInt64]$programHeaderCount = Read-U16 $bytes 56
    Assert-True -Condition (
        $programHeaderOffset + $programHeaderSize * $programHeaderCount -le [UInt64]$bytes.Length
    ) -Message "ELF program-header table is out of bounds"

    $headers = @()
    for ($index = 0; $index -lt $programHeaderCount; $index++) {
        [UInt64]$offset = $programHeaderOffset + [UInt64]$index * $programHeaderSize
        $header = [PSCustomObject]@{
            Kind = [UInt32](Read-U32 $bytes $offset)
            Flags = [UInt32](Read-U32 $bytes ($offset + 4))
            Offset = [UInt64](Read-U64 $bytes ($offset + 8))
            Vaddr = [UInt64](Read-U64 $bytes ($offset + 16))
            Paddr = [UInt64](Read-U64 $bytes ($offset + 24))
            FileSize = [UInt64](Read-U64 $bytes ($offset + 32))
            MemorySize = [UInt64](Read-U64 $bytes ($offset + 40))
            Alignment = [UInt64](Read-U64 $bytes ($offset + 48))
        }
        Assert-True -Condition ($header.FileSize -le $header.MemorySize) `
            -Message "ELF segment $index has file size greater than memory size"
        Assert-True -Condition ($header.Offset + $header.FileSize -le [UInt64]$bytes.Length) `
            -Message "ELF segment $index extends beyond the file"
        if ($header.Kind -eq 1) {
            Assert-True -Condition (Test-PowerOfTwo $header.Alignment) `
                -Message "ELF PT_LOAD $index has invalid alignment"
            Assert-Equal -Name "ELF PT_LOAD $index offset/vaddr congruence" `
                -Actual ($header.Offset % $header.Alignment) `
                -Expected ($header.Vaddr % $header.Alignment)
        }
        Assert-True -Condition (($header.Flags -band 3) -ne 3) `
            -Message "ELF segment $index is writable and executable"
        $headers += $header
    }

    Assert-True -Condition (@($headers | Where-Object { $_.Kind -eq 2 }).Count -eq 0) `
        -Message "static PIE unexpectedly has PT_DYNAMIC"
    Assert-True -Condition (@($headers | Where-Object { $_.Kind -eq 3 }).Count -eq 0) `
        -Message "static PIE unexpectedly has PT_INTERP"

    $loads = @($headers | Where-Object { $_.Kind -eq 1 })
    $headerLoads = @($loads | Where-Object { $_.Flags -eq 4 -and $_.Offset -eq 0 })
    $textLoads = @($loads | Where-Object { $_.Flags -eq 5 })
    $dataLoads = @($loads | Where-Object { $_.Flags -eq 6 })
    $metadataLoads = @($loads | Where-Object { $_.Flags -eq 4 -and $_.Offset -ne 0 })
    $stackHeaders = @($headers | Where-Object { $_.Kind -eq 0x6474e551 })
    Assert-Equal -Name "ELF PT_LOAD count" -Actual $loads.Count -Expected 4
    Assert-Equal -Name "ELF header R-- segment count" -Actual $headerLoads.Count -Expected 1
    Assert-Equal -Name "ELF text R-X segment count" -Actual $textLoads.Count -Expected 1
    Assert-Equal -Name "ELF data RW- segment count" -Actual $dataLoads.Count -Expected 1
    Assert-Equal -Name "ELF metadata R-- segment count" -Actual $metadataLoads.Count -Expected 1
    Assert-Equal -Name "ELF GNU-stack count" -Actual $stackHeaders.Count -Expected 1
    Assert-Equal -Name "ELF GNU-stack flags" -Actual $stackHeaders[0].Flags -Expected 6
    Assert-Equal -Name "ELF GNU-stack file size" -Actual $stackHeaders[0].FileSize -Expected 0
    Assert-Equal -Name "ELF GNU-stack memory size" -Actual $stackHeaders[0].MemorySize -Expected 0

    $orderedLoads = @($loads | Sort-Object Offset)
    for ($index = 1; $index -lt $orderedLoads.Count; $index++) {
        [UInt64]$priorEnd = $orderedLoads[$index - 1].Offset + $orderedLoads[$index - 1].FileSize
        Assert-True -Condition ($orderedLoads[$index].Offset -ge $priorEnd) `
            -Message "ELF load segments overlap in the file"
    }
    $orderedMemoryLoads = @($loads | Sort-Object Vaddr)
    for ($index = 1; $index -lt $orderedMemoryLoads.Count; $index++) {
        [UInt64]$priorEnd = $orderedMemoryLoads[$index - 1].Vaddr + $orderedMemoryLoads[$index - 1].MemorySize
        Assert-True -Condition ($orderedMemoryLoads[$index].Vaddr -ge $priorEnd) `
            -Message "ELF load segments overlap in memory"
    }

    [UInt64]$entry = Read-U64 $bytes 24
    Assert-True -Condition (
        $entry -ge $textLoads[0].Vaddr -and
        $entry -lt $textLoads[0].Vaddr + $textLoads[0].MemorySize
    ) -Message "ELF entrypoint is outside the R-X text segment"
    Assert-Equal -Name "ELF metadata is trailing" `
        -Actual ($metadataLoads[0].Offset + $metadataLoads[0].FileSize) `
        -Expected ([UInt64]$bytes.Length)

    Write-Host "PASS: segmented ET_DYN static PIE $Path"
    return [PSCustomObject]@{
        Path = $resolvedPath
        Bytes = $bytes
        Headers = $headers
        Header = $headerLoads[0]
        Text = $textLoads[0]
        Data = $dataLoads[0]
        Metadata = $metadataLoads[0]
        Entry = $entry
    }
}

function Get-ArcheEcsV2 {
    param(
        [Parameter(Mandatory = $true)]
        $Layout
    )

    [byte[]]$bytes = $Layout.Bytes
    [UInt64]$base = $Layout.Metadata.Offset
    [UInt64]$segmentLength = $Layout.Metadata.FileSize
    Assert-True -Condition ($segmentLength -ge 64) -Message "ARCHEECS metadata header is truncated"
    Assert-Equal -Name "ARCHEECS magic" `
        -Actual ([Text.Encoding]::ASCII.GetString($bytes, [int]$base, 8)) `
        -Expected "ARCHEECS"
    Assert-Equal -Name "ARCHEECS version" -Actual (Read-U32 $bytes ($base + 8)) -Expected 2
    Assert-Equal -Name "ARCHEECS header size" -Actual (Read-U32 $bytes ($base + 12)) -Expected 64
    Assert-Equal -Name "ARCHEECS header flags" -Actual (Read-U64 $bytes ($base + 16)) -Expected 0
    [UInt64]$totalLength = Read-U64 $bytes ($base + 24)
    [UInt64]$directoryOffset = Read-U64 $bytes ($base + 32)
    [UInt64]$directoryCount = Read-U64 $bytes ($base + 40)
    [UInt64]$directoryStride = Read-U64 $bytes ($base + 48)
    Assert-Equal -Name "ARCHEECS total length" -Actual $totalLength -Expected $segmentLength
    Assert-Equal -Name "ARCHEECS directory offset" -Actual $directoryOffset -Expected 64
    Assert-Equal -Name "ARCHEECS directory count" -Actual $directoryCount -Expected 14
    Assert-Equal -Name "ARCHEECS directory row size" -Actual $directoryStride -Expected 64
    Assert-Equal -Name "ARCHEECS reserved header" -Actual (Read-U64 $bytes ($base + 56)) -Expected 0
    Assert-True -Condition (
        $directoryOffset + $directoryCount * $directoryStride -le $totalLength
    ) -Message "ARCHEECS directory is out of bounds"

    [UInt64[]]$expectedStrides = @(0, 64, 96, 64, 128, 64, 80, 64, 64, 48, 64, 0, 96, 64)
    [UInt64]$cursor = $directoryOffset + $directoryCount * $directoryStride
    $sections = @()
    for ([UInt64]$index = 0; $index -lt $directoryCount; $index++) {
        [UInt64]$row = $base + $directoryOffset + $index * $directoryStride
        [UInt64]$kind = Read-U64 $bytes $row
        [UInt64]$flags = Read-U64 $bytes ($row + 8)
        [UInt64]$offset = Read-U64 $bytes ($row + 16)
        [UInt64]$byteLength = Read-U64 $bytes ($row + 24)
        [UInt64]$recordCount = Read-U64 $bytes ($row + 32)
        [UInt64]$recordStride = Read-U64 $bytes ($row + 40)
        [UInt64]$alignment = Read-U64 $bytes ($row + 48)
        [UInt64]$reserved = Read-U64 $bytes ($row + 56)

        Assert-Equal -Name "ARCHEECS section $index kind" -Actual $kind -Expected ($index + 1)
        Assert-Equal -Name "ARCHEECS section $kind flags" -Actual $flags -Expected 0
        Assert-Equal -Name "ARCHEECS section $kind reserved" -Actual $reserved -Expected 0
        Assert-Equal -Name "ARCHEECS section $kind alignment" -Actual $alignment -Expected 8
        Assert-Equal -Name "ARCHEECS section $kind stride" `
            -Actual $recordStride -Expected $expectedStrides[[int]$index]
        [UInt64]$expectedOffset = Align-Up $cursor $alignment
        Assert-Equal -Name "ARCHEECS section $kind canonical offset" `
            -Actual $offset -Expected $expectedOffset
        Assert-True -Condition ($offset + $byteLength -le $totalLength) `
            -Message "ARCHEECS section $kind is out of bounds"
        for ([UInt64]$padding = $cursor; $padding -lt $offset; $padding++) {
            Assert-Equal -Name "ARCHEECS zero padding at $padding" `
                -Actual $bytes[[int]($base + $padding)] -Expected 0
        }
        if ($recordStride -eq 0) {
            Assert-Equal -Name "ARCHEECS raw section $kind record count" `
                -Actual $recordCount -Expected 0
        }
        else {
            Assert-True -Condition ($recordCount -le [UInt64]::MaxValue / $recordStride) `
                -Message "ARCHEECS section $kind record shape overflows"
            Assert-Equal -Name "ARCHEECS section $kind fixed byte length" `
                -Actual $byteLength -Expected ($recordCount * $recordStride)
        }
        $sections += [PSCustomObject]@{
            Kind = $kind
            Offset = $offset
            ByteLength = $byteLength
            RecordCount = $recordCount
            RecordStride = $recordStride
            Alignment = $alignment
        }
        $cursor = $offset + $byteLength
    }
    Assert-Equal -Name "ARCHEECS canonical total length" -Actual $cursor -Expected $totalLength
    Assert-Equal -Name "ARCHEECS world record count" -Actual $sections[1].RecordCount -Expected 1

    foreach ($rawKind in @(1, 12)) {
        $section = @($sections | Where-Object { $_.Kind -eq $rawKind })[0]
        Assert-True -Condition ($section.ByteLength -ge 16) `
            -Message "ARCHEECS raw section $rawKind is shorter than its header"
        [UInt64]$rawBase = $base + $section.Offset
        [UInt64]$itemCount = Read-U64 $bytes $rawBase
        [UInt64]$dataLength = Read-U64 $bytes ($rawBase + 8)
        [UInt64]$rawRecordSize = if ($rawKind -eq 1) { 16 } else { 32 }
        Assert-True -Condition ($itemCount -le [UInt64]::MaxValue / $rawRecordSize) `
            -Message "ARCHEECS raw section $rawKind record range overflows"
        Assert-Equal -Name "ARCHEECS raw section $rawKind internal length" `
            -Actual $section.ByteLength `
            -Expected (16 + $itemCount * $rawRecordSize + $dataLength)
    }

    Write-Host "PASS: canonical ARCHEECS v2 envelope with 14 sections"
    return [PSCustomObject]@{
        Base = $base
        TotalLength = $totalLength
        DirectoryOffset = $directoryOffset
        Sections = $sections
    }
}

function Assert-CanonicalPayload {
    param(
        [Parameter(Mandatory = $true)]
        [UInt64] $Length,

        [Parameter(Mandatory = $true)]
        [string] $Payload,

        [Parameter(Mandatory = $true)]
        [string] $Context
    )

    if ($Length -eq 0) {
        Assert-Equal -Name "$Context empty payload" -Actual $Payload -Expected "-"
        return
    }
    Assert-True -Condition ($Length -le [int]::MaxValue) `
        -Message "$Context is too large for the proof host"
    Assert-True -Condition ($Payload -match '^[0-9A-F]+$') `
        -Message "$Context is not uppercase hexadecimal"
    Assert-Equal -Name "$Context hexadecimal length" `
        -Actual $Payload.Length -Expected ([int]$Length * 2)
}

function Test-ArcheObs2 {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string] $Text,

        [Parameter(Mandatory = $true)]
        [string] $Name
    )

    $normalized = $Text.Replace("`r`n", "`n").Replace("`r", "`n")
    Assert-True -Condition ($normalized.EndsWith("`n")) `
        -Message "$Name observation does not end with a newline"
    $lines = @($normalized.Substring(0, $normalized.Length - 1).Split([char]10))
    Assert-True -Condition ($lines.Count -ge 2) -Message "$Name observation is truncated"
    Assert-Equal -Name "$Name observation header" -Actual $lines[0] -Expected "ARCHEOBS2"
    Assert-Equal -Name "$Name observation terminator" -Actual $lines[$lines.Count - 1] -Expected "END"

    $lineIndex = 1
    $resourceCount = 0
    $initializedResourceCount = 0
    $uninitializedResourceCount = 0
    $zeroLengthPayloadCount = 0
    $priorResource = $null
    while ($lineIndex -lt $lines.Count - 1 -and $lines[$lineIndex].StartsWith("RESOURCE ")) {
        $line = $lines[$lineIndex]
        $uninitialized = [regex]::Match($line, '^RESOURCE ([0-9A-F]{32}) UNINITIALIZED$')
        $initialized = [regex]::Match(
            $line,
            '^RESOURCE ([0-9A-F]{32}) INITIALIZED ([0-9]+) (-|[0-9A-F]+)$'
        )
        Assert-True -Condition ($uninitialized.Success -or $initialized.Success) `
            -Message "$Name has invalid resource record: $line"
        $id = if ($uninitialized.Success) {
            $uninitialized.Groups[1].Value
        }
        else {
            $initialized.Groups[1].Value
        }
        if ($null -ne $priorResource) {
            Assert-True -Condition ([StringComparer]::Ordinal.Compare($priorResource, $id) -lt 0) `
                -Message "$Name resources are not in strict schema-ID order"
        }
        $priorResource = $id
        if ($uninitialized.Success) {
            $uninitializedResourceCount++
        }
        else {
            [UInt64]$length = [UInt64]::Parse($initialized.Groups[2].Value)
            Assert-CanonicalPayload -Length $length `
                -Payload $initialized.Groups[3].Value -Context "$Name resource $id"
            $initializedResourceCount++
            if ($length -eq 0) {
                $zeroLengthPayloadCount++
            }
        }
        $resourceCount++
        $lineIndex++
    }

    $tableCount = 0
    $rowCount = 0
    $columnCount = 0
    $priorTableKey = $null
    while ($lineIndex -lt $lines.Count - 1) {
        $parts = @($lines[$lineIndex].Split([char]32))
        Assert-True -Condition ($parts.Count -ge 3 -and $parts[0] -eq "TABLE") `
            -Message "$Name expected TABLE at line $($lineIndex + 1)"
        [UInt64]$keyCount = [UInt64]::Parse($parts[1])
        Assert-Equal -Name "$Name table token count" `
            -Actual $parts.Count -Expected ([int]$keyCount + 3)
        $keyIds = @()
        for ($keyIndex = 0; $keyIndex -lt $keyCount; $keyIndex++) {
            $id = $parts[2 + $keyIndex]
            Assert-True -Condition ($id -match '^[0-9A-F]{32}$') `
                -Message "$Name has invalid table schema ID '$id'"
            if ($keyIndex -gt 0) {
                Assert-True -Condition (
                    [StringComparer]::Ordinal.Compare($keyIds[$keyIndex - 1], $id) -lt 0
                ) -Message "$Name table key is not sorted"
            }
            $keyIds += $id
        }
        [UInt64]$declaredRows = [UInt64]::Parse($parts[$parts.Count - 1])
        $tableKey = $keyIds -join ":"
        if ($null -ne $priorTableKey) {
            Assert-True -Condition ([StringComparer]::Ordinal.Compare($priorTableKey, $tableKey) -lt 0) `
                -Message "$Name tables are not in canonical key order"
        }
        $priorTableKey = $tableKey
        $lineIndex++

        $priorSpawn = $null
        for ([UInt64]$expectedRow = 0; $expectedRow -lt $declaredRows; $expectedRow++) {
            Assert-True -Condition ($lineIndex -lt $lines.Count - 1) `
                -Message "$Name table row list is truncated"
            $rowParts = @($lines[$lineIndex].Split([char]32))
            Assert-True -Condition ($rowParts.Count -eq 4 -and $rowParts[0] -eq "ROW") `
                -Message "$Name has invalid row record '$($lines[$lineIndex])'"
            Assert-Equal -Name "$Name row index" `
                -Actual ([UInt64]::Parse($rowParts[1])) -Expected $expectedRow
            [UInt64]$spawnOrdinal = [UInt64]::Parse($rowParts[2])
            if ($null -ne $priorSpawn) {
                Assert-True -Condition ($spawnOrdinal -gt $priorSpawn) `
                    -Message "$Name row spawn ordinals are not committed-order increasing"
            }
            $priorSpawn = $spawnOrdinal
            [UInt64]$declaredColumns = [UInt64]::Parse($rowParts[3])
            Assert-Equal -Name "$Name row column count" -Actual $declaredColumns -Expected $keyCount
            $lineIndex++

            for ([UInt64]$columnIndex = 0; $columnIndex -lt $declaredColumns; $columnIndex++) {
                Assert-True -Condition ($lineIndex -lt $lines.Count - 1) `
                    -Message "$Name row column list is truncated"
                $column = [regex]::Match(
                    $lines[$lineIndex],
                    '^COLUMN ([0-9A-F]{32}) ([0-9]+) (-|[0-9A-F]+)$'
                )
                Assert-True -Condition $column.Success `
                    -Message "$Name has invalid column record '$($lines[$lineIndex])'"
                Assert-Equal -Name "$Name column schema order" `
                    -Actual $column.Groups[1].Value -Expected $keyIds[[int]$columnIndex]
                [UInt64]$length = [UInt64]::Parse($column.Groups[2].Value)
                Assert-CanonicalPayload -Length $length `
                    -Payload $column.Groups[3].Value `
                    -Context "$Name column $($column.Groups[1].Value)"
                if ($length -eq 0) {
                    $zeroLengthPayloadCount++
                }
                $columnCount++
                $lineIndex++
            }
            $rowCount++
        }
        $tableCount++
    }

    Assert-Equal -Name "$Name complete observation consumption" `
        -Actual $lineIndex -Expected ($lines.Count - 1)
    Write-Host "PASS: canonical ARCHEOBS2 $Name"
    return [PSCustomObject]@{
        Resources = $resourceCount
        InitializedResources = $initializedResourceCount
        UninitializedResources = $uninitializedResourceCount
        Tables = $tableCount
        Rows = $rowCount
        Columns = $columnCount
        ZeroLengthPayloads = $zeroLengthPayloadCount
    }
}

function ConvertTo-WslPath {
    param([string] $Path)
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    Assert-True -Condition ($resolved -match '^([A-Za-z]):\\(.*)$') `
        -Message "cannot translate path to WSL: $resolved"
    return "/mnt/$($matches[1].ToLowerInvariant())/$($matches[2].Replace('\', '/'))"
}

function Invoke-LinuxArtifact {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    if ($SkipGeneratedLinuxExecution) {
        Write-Host "SKIP: $Name generated Linux execution was explicitly disabled"
        return $null
    }
    if ($isLinuxPlatform) {
        return Invoke-CapturedProcess -Name $Name `
            -Executable (Resolve-Path -LiteralPath $Path).Path
    }
    if ($isWindowsPlatform) {
        Assert-True -Condition ($null -ne (Get-Command wsl.exe -ErrorAction SilentlyContinue)) `
            -Message "WSL is required unless -SkipGeneratedLinuxExecution is supplied"
        return Invoke-CapturedProcess -Name $Name -Executable "wsl.exe" `
            -Arguments @((ConvertTo-WslPath $Path))
    }
    throw "generated Linux execution requires Linux or WSL; use -SkipGeneratedLinuxExecution"
}

function Test-RepeatedArtifactExecution {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [int] $ExpectedStatus,

        [Parameter(Mandatory = $false)]
        [switch] $ExpectTrap
    )

    $first = Invoke-LinuxArtifact -Name "$Name execution 1" -Path $Path
    if ($null -eq $first) {
        return $null
    }
    Assert-ProcessStatus -Result $first -Expected $ExpectedStatus
    $summary = Test-ArcheObs2 -Text $first.Stdout -Name $Name
    if (!$ExpectTrap) {
        Assert-Equal -Name "$Name stderr" -Actual $first.Stderr -Expected ""
    }

    for ($attempt = 2; $attempt -le 3; $attempt++) {
        $next = Invoke-LinuxArtifact -Name "$Name execution $attempt" -Path $Path
        Assert-ProcessStatus -Result $next -Expected $ExpectedStatus
        Assert-Equal -Name "$Name ASLR stdout $attempt" -Actual $next.Stdout -Expected $first.Stdout
        Assert-Equal -Name "$Name ASLR stderr $attempt" -Actual $next.Stderr -Expected $first.Stderr
    }
    Write-Host "PASS: $Name is byte-stable across repeated ASLR executions"
    return [PSCustomObject]@{ Result = $first; Summary = $summary }
}

function Assert-NoCompleteObservation {
    param([string] $Name, [string] $Stdout)
    $normalized = Normalize-LineEndings $Stdout
    Assert-True -Condition ($normalized -notmatch '(?m)^ARCHEOBS2$') `
        -Message "$Name emitted an observation header before metadata rejection"
    Assert-True -Condition ($normalized -notmatch '(?m)^END$') `
        -Message "$Name emitted an END record before metadata rejection"
}

function Ensure-ExecutableCopy {
    param([string] $Path)
    if ($isLinuxPlatform) {
        $chmod = Invoke-CapturedProcess -Name "mark corrupt fixture executable" `
            -Executable "chmod" -Arguments @("u+x", $Path)
        Assert-ProcessStatus -Result $chmod -Expected 0
    }
}

function Test-MetadataRejections {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Artifact,

        [Parameter(Mandatory = $true)]
        $Layout,

        [Parameter(Mandatory = $true)]
        $Package
    )

    if ($SkipGeneratedLinuxExecution) {
        Write-Host "SKIP: metadata rejection execution was explicitly disabled"
        return
    }

    $cases = @()

    $v1Path = Join-Path $proofRoot "m26-v1"
    [byte[]]$v1Bytes = [System.IO.File]::ReadAllBytes($Layout.Path)
    Write-U32 -Bytes $v1Bytes -Offset ($Package.Base + 8) -Value 1
    [System.IO.File]::WriteAllBytes($v1Path, $v1Bytes)
    Ensure-ExecutableCopy $v1Path
    $cases += [PSCustomObject]@{
        Name = "ARCHEECS version 1"
        Path = $v1Path
        Diagnostic = "arche: unsupported ARCHEECS version 1; rebuild with archec0"
    }

    $componentPath = Join-Path $proofRoot "m26-archecmp"
    [byte[]]$componentBytes = [System.IO.File]::ReadAllBytes($Layout.Path)
    [Text.Encoding]::ASCII.GetBytes("ARCHECMP").CopyTo($componentBytes, [int]$Package.Base)
    [System.IO.File]::WriteAllBytes($componentPath, $componentBytes)
    Ensure-ExecutableCopy $componentPath
    $cases += [PSCustomObject]@{
        Name = "ARCHECMP artifact"
        Path = $componentPath
        Diagnostic = "arche: unsupported ARCHECMP artifact; rebuild with archec0"
    }

    $directoryPath = Join-Path $proofRoot "m26-bad-directory"
    [byte[]]$directoryBytes = [System.IO.File]::ReadAllBytes($Layout.Path)
    Write-U64 -Bytes $directoryBytes `
        -Offset ($Package.Base + $Package.DirectoryOffset + 8) -Value 1
    [System.IO.File]::WriteAllBytes($directoryPath, $directoryBytes)
    Ensure-ExecutableCopy $directoryPath
    $cases += [PSCustomObject]@{
        Name = "ARCHEECS nonzero directory flags"
        Path = $directoryPath
        Diagnostic = $null
    }

    $functionSection = @($Package.Sections | Where-Object { $_.Kind -eq 13 })[0]
    Assert-True -Condition ($functionSection.RecordCount -gt 0) `
        -Message "fixture has no function link to corrupt"
    $functionPath = Join-Path $proofRoot "m26-bad-function-link"
    [byte[]]$functionBytes = [System.IO.File]::ReadAllBytes($Layout.Path)
    Write-U64 -Bytes $functionBytes `
        -Offset ($Package.Base + $functionSection.Offset + 56) `
        -Value ([UInt64]::MaxValue)
    [System.IO.File]::WriteAllBytes($functionPath, $functionBytes)
    Ensure-ExecutableCopy $functionPath
    $cases += [PSCustomObject]@{
        Name = "ARCHEECS invalid function offset"
        Path = $functionPath
        Diagnostic = $null
    }

    foreach ($case in $cases) {
        $result = Invoke-LinuxArtifact -Name $case.Name -Path $case.Path
        Assert-ProcessStatus -Result $result -Expected 1
        Assert-NoCompleteObservation -Name $case.Name -Stdout $result.Stdout
        if ($null -ne $case.Diagnostic) {
            $diagnostic = Normalize-LineEndings $result.Stderr
            Assert-Equal -Name "$($case.Name) rebuild diagnostic" -Actual $diagnostic `
                -Expected "$($case.Diagnostic)`n"
        }
    }
    Write-Host "PASS: malformed v2 links, ARCHEECS v1, and ARCHECMP fail before world mutation without END"
}

function Test-NoSiblingTemporaries {
    param([string] $Directory)
    $temporaries = @(Get-ChildItem -LiteralPath $Directory -Force -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.Name.Contains(".archec0-tmp-") })
    Assert-Equal -Name "sibling temporary cleanup" -Actual $temporaries.Count -Expected 0
}

function Test-PublicationContracts {
    param([string] $Compiler)

    $publicationRoot = Join-Path $proofRoot "publication"
    [System.IO.Directory]::CreateDirectory($publicationRoot) | Out-Null
    $source = Join-Path $publicationRoot "source.arc"
    [System.IO.File]::Copy((Join-Path $repoRoot "examples/exit42.arc"), $source, $true)
    [byte[]]$original = [System.IO.File]::ReadAllBytes($source)

    $alias = Invoke-Compiler -Compiler $Compiler -Name "reject exact source/output alias" `
        -Arguments @($source, "-o", $source)
    Assert-ProcessStatus -Result $alias -Expected 2
    Assert-Equal -Name "source alias diagnostic" `
        -Actual (Normalize-LineEndings $alias.Stderr) `
        -Expected "archec0: refusing to overwrite input source with output $source`n"
    Assert-Equal -Name "source/output alias preserves source" `
        -Actual ([Convert]::ToBase64String([System.IO.File]::ReadAllBytes($source))) `
        -Expected ([Convert]::ToBase64String($original))

    $hardLink = Join-Path $publicationRoot "source-hard-link.arc"
    New-Item -ItemType HardLink -Path $hardLink -Target $source | Out-Null
    $hardAlias = Invoke-Compiler -Compiler $Compiler -Name "reject hard-link source alias" `
        -Arguments @($source, "-o", $hardLink)
    Assert-ProcessStatus -Result $hardAlias -Expected 2
    Assert-Equal -Name "hard-link source alias diagnostic" `
        -Actual (Normalize-LineEndings $hardAlias.Stderr) `
        -Expected "archec0: refusing to overwrite input source with output $hardLink`n"
    Remove-Item -LiteralPath $hardLink -Force

    if ($isLinuxPlatform) {
        $symbolicLink = Join-Path $publicationRoot "source-symbolic-link.arc"
        New-Item -ItemType SymbolicLink -Path $symbolicLink -Target $source | Out-Null
        $symbolicAlias = Invoke-Compiler -Compiler $Compiler `
            -Name "reject symbolic-link source alias" `
            -Arguments @($source, "-o", $symbolicLink)
        Assert-ProcessStatus -Result $symbolicAlias -Expected 2
        Assert-Equal -Name "symbolic-link source alias diagnostic" `
            -Actual (Normalize-LineEndings $symbolicAlias.Stderr) `
            -Expected "archec0: refusing to overwrite input source with output $symbolicLink`n"
        Remove-Item -LiteralPath $symbolicLink -Force
    }
    else {
        Write-Host "SKIP: black-box symbolic-link alias creation is Unix-only; Rust unit coverage remains required"
    }

    $replace = Join-Path $publicationRoot "replace"
    [System.IO.File]::WriteAllText($replace, "old artifact", $utf8NoBom)
    $replaceResult = Invoke-Compiler -Compiler $Compiler -Name "atomic replacement publication" `
        -Arguments @($source, "-o", $replace)
    Assert-ProcessStatus -Result $replaceResult -Expected 0
    [byte[]]$replaceBytes = [System.IO.File]::ReadAllBytes($replace)
    Assert-Equal -Name "atomic replacement ELF prefix" -Actual $replaceBytes[0] -Expected 0x7f

    $blocked = Join-Path $publicationRoot "existing-directory"
    [System.IO.Directory]::CreateDirectory($blocked) | Out-Null
    $sentinel = Join-Path $blocked "sentinel"
    [System.IO.File]::WriteAllText($sentinel, "preserved", $utf8NoBom)
    $blockedResult = Invoke-Compiler -Compiler $Compiler `
        -Name "publication failure preserves directory target" `
        -Arguments @($source, "-o", $blocked)
    Assert-ProcessStatus -Result $blockedResult -Expected 1
    Assert-Equal -Name "publication failure sentinel" `
        -Actual ([System.IO.File]::ReadAllText($sentinel)) -Expected "preserved"
    Test-NoSiblingTemporaries $publicationRoot

    if ($isLinuxPlatform) {
        $mode = Invoke-CapturedProcess -Name "published executable permission" `
            -Executable "stat" -Arguments @("-c", "%a", $replace)
        Assert-ProcessStatus -Result $mode -Expected 0
        $modeText = $mode.Stdout.Trim()
        Assert-True -Condition ($modeText -match '^[1357][0-7][0-7]$') `
            -Message "published output lacks the owner execute bit"
    }
    else {
        Write-Host "SKIP: Unix executable-mode black-box check on Windows host"
    }
    Write-Host "PASS: alias, atomic replacement, permission, and cleanup contracts"
}

function Test-CliModes {
    param([string] $Compiler)

    $help = Invoke-Compiler -Compiler $Compiler -Name "CLI help" -Arguments @("--help")
    Assert-ProcessStatus $help 0
    Assert-Contains -Name "CLI help bare/check alias" -Actual $help.Stdout `
        -Expected "Source-only invocation is equivalent to --check."
    $version = Invoke-Compiler -Compiler $Compiler -Name "CLI version" -Arguments @("--version")
    Assert-ProcessStatus $version 0
    Assert-Contains -Name "CLI version" -Actual $version.Stdout -Expected "archec0 "
    $noInput = Invoke-Compiler $Compiler "CLI no input" @()
    Assert-ProcessStatus $noInput 2
    Assert-Equal -Name "CLI no-input diagnostic" `
        -Actual (Normalize-LineEndings $noInput.Stderr) `
        -Expected "archec0: no input provided`nrun ``archec0 --help`` for usage`n"
    $unsupported = Invoke-Compiler $Compiler "CLI unsupported arguments" `
        @("one", "two", "three", "four")
    Assert-ProcessStatus $unsupported 2
    Assert-Equal -Name "CLI unsupported-arguments diagnostic" `
        -Actual (Normalize-LineEndings $unsupported.Stderr) `
        -Expected "archec0: command not implemented yet`nrun ``archec0 --help`` for usage`n"
    $missingSource = Invoke-Compiler $Compiler "CLI missing source" `
        @("does-not-exist.arc", "--check")
    Assert-ProcessStatus $missingSource 2
    Assert-Equal -Name "CLI missing-source diagnostic" `
        -Actual (Normalize-LineEndings $missingSource.Stderr) `
        -Expected "archec0: source file not found: does-not-exist.arc`n"

    $exit42 = Join-Path $repoRoot "examples/exit42.arc"
    $bare = Invoke-Compiler $Compiler "bare executable check" @($exit42)
    $explicit = Invoke-Compiler $Compiler "explicit executable check" @($exit42, "--check")
    Assert-ProcessStatus $bare 0
    Assert-ProcessStatus $explicit 0
    $expectedCheck = "archec0: check passed $exit42`n"
    Assert-Equal -Name "bare check result" `
        -Actual (Normalize-LineEndings $bare.Stdout) -Expected $expectedCheck
    Assert-Equal -Name "explicit check result" `
        -Actual (Normalize-LineEndings $explicit.Stdout) -Expected $expectedCheck
    Assert-Equal -Name "bare check stderr" -Actual $bare.Stderr -Expected ""
    Assert-Equal -Name "explicit check stderr" -Actual $explicit.Stderr -Expected ""

    $declarations = Join-Path $proofRoot "declaration-only.arc"
    Write-Utf8File $declarations @"
world DeclarationOnly
component Marker {}
tag Visible
resource Config { enabled: bool }
"@
    Assert-ProcessStatus (Invoke-Compiler $Compiler "syntax-only AST mode" @($declarations, "--emit-ast")) 0
    Assert-ProcessStatus (Invoke-Compiler $Compiler "syntax-only token mode" @($declarations, "--emit-tokens")) 0
    Assert-ProcessStatus (Invoke-Compiler $Compiler "declaration-only inspection" @($declarations, "--inspect-components")) 0
    $expectedMissingStartup = "${declarations}:4:34: error[CHECK001]: executable program requires a ``startup`` block`n"
    $bareMissingStartup = Invoke-Compiler $Compiler "bare mode requires startup" @($declarations)
    $checkMissingStartup = Invoke-Compiler $Compiler "check mode requires startup" `
        @($declarations, "--check")
    $coreMissingStartup = Invoke-Compiler $Compiler "Core mode requires startup" `
        @($declarations, "--emit-core")
    $machineMissingStartup = Invoke-Compiler $Compiler "Machine mode requires startup" `
        @($declarations, "--emit-machine")
    foreach ($result in @(
        $bareMissingStartup,
        $checkMissingStartup,
        $coreMissingStartup,
        $machineMissingStartup
    )) {
        Assert-ProcessStatus $result 1
        Assert-Equal -Name "$($result.Name) diagnostic" `
            -Actual (Normalize-LineEndings $result.Stderr) `
            -Expected $expectedMissingStartup
    }
    $missingOutput = Join-Path $proofRoot "declaration-only-output"
    $outputMissingStartup = Invoke-Compiler $Compiler "output mode requires startup" `
        @($declarations, "-o", $missingOutput)
    Assert-ProcessStatus $outputMissingStartup 1
    Assert-Equal -Name "output mode missing-startup diagnostic" `
        -Actual (Normalize-LineEndings $outputMissingStartup.Stderr) `
        -Expected $expectedMissingStartup
    Assert-True -Condition (!(Test-Path -LiteralPath $missingOutput)) `
        -Message "failed executable check published an output"

    $duplicateStartup = Join-Path $proofRoot "duplicate-startup.arc"
    Write-Utf8File $duplicateStartup @"
world DuplicateStartup
startup { exit 0 }
component Marker {}
startup { exit 1 }
"@
    $duplicateAst = Invoke-Compiler $Compiler "duplicate startup syntax-only AST" `
        @($duplicateStartup, "--emit-ast")
    Assert-ProcessStatus $duplicateAst 0
    Assert-Equal -Name "syntax-only AST retains both startup blocks" `
        -Actual ([regex]::Matches($duplicateAst.Stdout, "(?m)^  startup$").Count) -Expected 2
    Assert-ProcessStatus `
        (Invoke-Compiler $Compiler "duplicate startup declaration inspection" `
            @($duplicateStartup, "--inspect-components")) 0

    $expectedDuplicateStartup = "${duplicateStartup}:4:1: error[CHECK001]: multiple ``startup`` blocks are not allowed`n"
    foreach ($result in @(
        (Invoke-Compiler $Compiler "bare mode rejects duplicate startup" @($duplicateStartup)),
        (Invoke-Compiler $Compiler "check mode rejects duplicate startup" `
            @($duplicateStartup, "--check")),
        (Invoke-Compiler $Compiler "Core mode rejects duplicate startup" `
            @($duplicateStartup, "--emit-core")),
        (Invoke-Compiler $Compiler "Machine mode rejects duplicate startup" `
            @($duplicateStartup, "--emit-machine"))
    )) {
        Assert-ProcessStatus $result 1
        Assert-Equal -Name "$($result.Name) diagnostic" `
            -Actual (Normalize-LineEndings $result.Stderr) `
            -Expected $expectedDuplicateStartup
    }
    $duplicateOutput = Join-Path $proofRoot "duplicate-startup-output"
    $duplicateOutputResult = Invoke-Compiler $Compiler "output mode rejects duplicate startup" `
        @($duplicateStartup, "-o", $duplicateOutput)
    Assert-ProcessStatus $duplicateOutputResult 1
    Assert-Equal -Name "output mode duplicate-startup diagnostic" `
        -Actual (Normalize-LineEndings $duplicateOutputResult.Stderr) `
        -Expected $expectedDuplicateStartup
    Assert-True -Condition (!(Test-Path -LiteralPath $duplicateOutput)) `
        -Message "duplicate-startup executable check published an output"

    $badSyntax = Join-Path $repoRoot "tests/e2e/bad_syntax.arc"
    $syntaxFailure = Invoke-Compiler $Compiler "parse failure status" @($badSyntax, "--emit-ast")
    Assert-ProcessStatus $syntaxFailure 1
    Assert-Equal -Name "parse failure diagnostic" `
        -Actual (Normalize-LineEndings $syntaxFailure.Stderr) `
        -Expected "${badSyntax}:5:1: error[PARSE001]: expected expression after ``exit```n"

    foreach ($fixture in @(
        "bad_i32_arithmetic.arc",
        "bad_unknown_schedule_run.arc",
        "bad_unknown_resource_param.arc",
        "bad_unknown_query_component.arc",
        "bad_conflicting_query_access.arc"
    )) {
        $path = Join-Path $repoRoot "tests/e2e/$fixture"
        $failure = Invoke-Compiler $Compiler "semantic rejection $fixture" @($path, "--check")
        Assert-ProcessStatus $failure 1
        Assert-Contains -Name "$fixture diagnostic path" -Actual $failure.Stderr -Expected $fixture
        Assert-Contains -Name "$fixture diagnostic code" -Actual $failure.Stderr `
            -Expected "error[CHECK001]"
    }
    Write-Host "PASS: CLI mode and status boundaries"
}

function Assert-ProofTestInventory {
    param($TestList)

    foreach ($portable in @(
        "parser::tests::reports_an_incomplete_startup_literal_at_captured_eof",
        "checker::tests::rejects_executable_without_startup",
        "checker::tests::rejects_startup_without_final_exit",
        "reference_executor_v2::tests::executes_the_primary_m26_closure_fixture_from_decoded_v2_metadata",
        "reference_executor_v2::tests::executes_the_external_m26_trap_fixture_with_committed_state",
        "reference_executor_v2::tests::source_exit_uses_the_low_eight_bits_without_a_trap_diagnostic",
        "reference_executor_v2::tests::every_integer_trap_edge_preserves_prior_commits_and_skips_the_trapping_spawn",
        "output::tests::rejects_exact_and_relative_source_aliases",
        "output::tests::rejects_hard_link_source_alias",
        "output::tests::producer_failure_preserves_existing_artifact_and_cleans_temporary"
    )) {
        Assert-Contains -Name "required Rust proof" -Actual $TestList.Stdout -Expected $portable
    }
    if ($isLinuxPlatform) {
        Assert-Contains -Name "closure reference/native proof" -Actual $TestList.Stdout `
            -Expected "aot_v2::tests::rich_m26_native_matches_direct_core_reference"
        Assert-True -Condition ($TestList.Stdout -match '(?im)^.*arena.*native.*reference.*: test$|^.*native.*arena.*reference.*: test$') `
            -Message "M26 red gate: no Arena direct-Core/native parity test is registered"
        Assert-Contains -Name "trap reference/native proof" -Actual $TestList.Stdout `
            -Expected "aot_v2::tests::trap_native_matches_exact_direct_core_observation_and_diagnostic"
    }
    Write-Host "PASS: M26 reference/native proof inventory"
}

Push-Location $repoRoot
try {
    Write-Host "PowerShell host: $($PSVersionTable.PSEdition) $($PSVersionTable.PSVersion)"
    if (Test-Path -LiteralPath $proofRoot) {
        Remove-Item -LiteralPath $proofRoot -Recurse -Force
    }
    [System.IO.Directory]::CreateDirectory($proofRoot) | Out-Null

    $tests = Invoke-CapturedProcess -Name "locked debug workspace all-target Rust tests" `
        -Executable "cargo" `
        -Arguments @("test", "--locked", "--workspace", "--all-targets", "--manifest-path", $manifestPath)
    Assert-ProcessStatus $tests 0
    $build = Invoke-CapturedProcess -Name "locked debug workspace build" `
        -Executable "cargo" `
        -Arguments @("build", "--locked", "--workspace", "--manifest-path", $manifestPath)
    Assert-ProcessStatus $build 0

    $compilerName = if ($isWindowsPlatform) { "archec0.exe" } else { "archec0" }
    $compiler = Join-Path $repoRoot "bootstrap/archec0/target/debug/$compilerName"
    Assert-True -Condition (Test-Path -LiteralPath $compiler -PathType Leaf) `
        -Message "compiler was not built at $compiler"

    $publicCliName = if ($isWindowsPlatform) { "arche.exe" } else { "arche" }
    $publicCli = Join-Path $repoRoot "bootstrap/archec0/target/debug/$publicCliName"
    Assert-True -Condition (Test-Path -LiteralPath $publicCli -PathType Leaf) `
        -Message "public CLI was not built at $publicCli"
    $publicHelp = Invoke-CapturedProcess -Name "public CLI help" `
        -Executable $publicCli -Arguments @("--help")
    Assert-ProcessStatus $publicHelp 0
    Assert-Equal -Name "public CLI help stderr" -Actual $publicHelp.Stderr -Expected ""
    Assert-Contains -Name "public CLI command inventory" `
        -Actual $publicHelp.Stdout -Expected "M27 commands:"
    $publicVersion = Invoke-CapturedProcess -Name "public CLI version" `
        -Executable $publicCli -Arguments @("--version")
    Assert-ProcessStatus $publicVersion 0
    Assert-Contains -Name "public CLI version text" `
        -Actual $publicVersion.Stdout -Expected "arche 0.0.0"
    $publicReserved = Invoke-CapturedProcess -Name "reserved public command" `
        -Executable $publicCli -Arguments @("build")
    Assert-ProcessStatus $publicReserved 2
    Assert-Equal -Name "reserved command stdout" `
        -Actual $publicReserved.Stdout -Expected ""
    Assert-Contains -Name "reserved command diagnostic" `
        -Actual $publicReserved.Stderr -Expected "reserved but not implemented yet"
    $publicUnknown = Invoke-CapturedProcess -Name "unknown public command" `
        -Executable $publicCli -Arguments @("not-a-command")
    Assert-ProcessStatus $publicUnknown 2
    Assert-Contains -Name "unknown command diagnostic" `
        -Actual $publicUnknown.Stderr -Expected "unknown command ``not-a-command``"

    Test-M27BPublicCheck -PublicCli $publicCli

    Test-CliModes $compiler
    Test-PublicationContracts $compiler

    $exit42Artifact = Join-Path $proofRoot "exit42"
    $exit42Compile = Invoke-Compiler $compiler "compile exit42 v2 PIE" `
        @((Join-Path $repoRoot "examples/exit42.arc"), "-o", $exit42Artifact)
    Assert-ProcessStatus $exit42Compile 0
    $exit42Layout = Get-StaticPieLayout $exit42Artifact
    $null = Get-ArcheEcsV2 $exit42Layout
    $exit42Run = Test-RepeatedArtifactExecution "exit42" $exit42Artifact 42
    if ($null -ne $exit42Run) {
        Assert-Equal -Name "exit42 resources" -Actual $exit42Run.Summary.Resources -Expected 0
        Assert-Equal -Name "exit42 tables" -Actual $exit42Run.Summary.Tables -Expected 0
    }

    $exit70Source = Join-Path $proofRoot "exit70.arc"
    Write-Utf8File $exit70Source @"
world ExitSeventy
startup { exit 70 }
"@
    $exit70Artifact = Join-Path $proofRoot "exit70"
    Assert-ProcessStatus (Invoke-Compiler $compiler "compile source exit 70" `
        @($exit70Source, "-o", $exit70Artifact)) 0
    $exit70Layout = Get-StaticPieLayout $exit70Artifact
    $null = Get-ArcheEcsV2 $exit70Layout
    $exit70Run = Test-RepeatedArtifactExecution "source_exit_70" $exit70Artifact 70
    if ($null -ne $exit70Run) {
        Assert-Equal -Name "source exit 70 resources" `
            -Actual $exit70Run.Summary.Resources -Expected 0
        Assert-Equal -Name "source exit 70 tables" `
            -Actual $exit70Run.Summary.Tables -Expected 0
        Assert-Equal -Name "source exit 70 trap diagnostic absence" `
            -Actual $exit70Run.Result.Stderr -Expected ""
    }

    $closureSource = Join-Path $repoRoot "examples/m26_closure.arc"
    $closureAstGolden = [System.IO.File]::ReadAllText(
        (Join-Path $repoRoot "tests/golden/m26_closure.ast")
    )
    $closureCoreGolden = [System.IO.File]::ReadAllText(
        (Join-Path $repoRoot "tests/golden/m26_closure.core")
    )
    $closureMachineGolden = [System.IO.File]::ReadAllText(
        (Join-Path $repoRoot "tests/golden/m26_closure.machine")
    )
    $closureAst = Invoke-Compiler $compiler "M26 closure AST emission" @($closureSource, "--emit-ast")
    Assert-ProcessStatus $closureAst 0
    Assert-Equal -Name "M26 closure exact AST golden" `
        -Actual (Normalize-LineEndings $closureAst.Stdout) `
        -Expected (Normalize-LineEndings $closureAstGolden)
    $closureCore1 = Invoke-Compiler $compiler "M26 closure Core emission 1" @($closureSource, "--emit-core")
    $closureCore2 = Invoke-Compiler $compiler "M26 closure Core emission 2" @($closureSource, "--emit-core")
    Assert-ProcessStatus $closureCore1 0
    Assert-ProcessStatus $closureCore2 0
    Assert-Equal -Name "M26 closure deterministic Core" -Actual $closureCore2.Stdout -Expected $closureCore1.Stdout
    Assert-Equal -Name "M26 closure exact Core golden" `
        -Actual (Normalize-LineEndings $closureCore1.Stdout) `
        -Expected (Normalize-LineEndings $closureCoreGolden)
    Assert-Contains -Name "M26 closure Core world" -Actual $closureCore1.Stdout -Expected "world M26Closure"
    Assert-Contains -Name "M26 closure Core system" -Actual $closureCore1.Stdout -Expected "system Advance"
    $closureMachine = Invoke-Compiler $compiler "M26 closure Machine emission" @($closureSource, "--emit-machine")
    Assert-ProcessStatus $closureMachine 0
    Assert-Equal -Name "M26 closure exact Machine golden" `
        -Actual (Normalize-LineEndings $closureMachine.Stdout) `
        -Expected (Normalize-LineEndings $closureMachineGolden)
    Assert-Contains -Name "M26 closure Machine startup" -Actual $closureMachine.Stdout -Expected "function startup"

    $closureArtifact = Join-Path $proofRoot "m26-closure"
    Assert-ProcessStatus (Invoke-Compiler $compiler "compile primary M26 closure" `
        @($closureSource, "-o", $closureArtifact)) 0
    $closureLayout = Get-StaticPieLayout $closureArtifact
    $closurePackage = Get-ArcheEcsV2 $closureLayout
    $closureRun = Test-RepeatedArtifactExecution "m26_closure" $closureArtifact 47
    if ($null -ne $closureRun) {
        Assert-Equal -Name "M26 closure resources" -Actual $closureRun.Summary.Resources -Expected 5
        Assert-Equal -Name "M26 closure initialized resources" -Actual $closureRun.Summary.InitializedResources -Expected 3
        Assert-Equal -Name "M26 closure uninitialized resources" -Actual $closureRun.Summary.UninitializedResources -Expected 2
        Assert-Equal -Name "M26 closure tables" -Actual $closureRun.Summary.Tables -Expected 4
        Assert-Equal -Name "M26 closure rows" -Actual $closureRun.Summary.Rows -Expected 4
        Assert-True -Condition ($closureRun.Summary.ZeroLengthPayloads -gt 0) `
            -Message "M26 closure observation omitted tags, ZSTs, or the initialized empty resource"
    }

    $arenaSource = Join-Path $repoRoot "examples/arena_recovery.arc"
    $arenaArtifact = Join-Path $proofRoot "arena-recovery"
    Assert-ProcessStatus (Invoke-Compiler $compiler "compile structurally distinct Arena" `
        @($arenaSource, "-o", $arenaArtifact)) 0
    $arenaLayout = Get-StaticPieLayout $arenaArtifact
    $null = Get-ArcheEcsV2 $arenaLayout
    $arenaRun = Test-RepeatedArtifactExecution "arena_recovery" $arenaArtifact 0
    if ($null -ne $arenaRun) {
        Assert-Equal -Name "Arena resources" -Actual $arenaRun.Summary.Resources -Expected 1
        Assert-Equal -Name "Arena tables" -Actual $arenaRun.Summary.Tables -Expected 2
        Assert-Equal -Name "Arena rows" -Actual $arenaRun.Summary.Rows -Expected 5
        Assert-Equal -Name "Arena columns" -Actual $arenaRun.Summary.Columns -Expected 13
    }

    $trapSource = Join-Path $repoRoot "examples/m26_trap.arc"
    $trapArtifact = Join-Path $proofRoot "m26-trap"
    Assert-ProcessStatus (Invoke-Compiler $compiler "compile M26 trap fixture" `
        @($trapSource, "-o", $trapArtifact)) 0
    $trapLayout = Get-StaticPieLayout $trapArtifact
    $null = Get-ArcheEcsV2 $trapLayout
    $trapRun = Test-RepeatedArtifactExecution "m26_trap" $trapArtifact 70 -ExpectTrap
    if ($null -ne $trapRun) {
        $trapDiagnostic = Normalize-LineEndings $trapRun.Result.Stderr
        $trapText = [System.IO.File]::ReadAllText($trapSource)
        $trapExpression = "counter.value / denominator.value"
        $trapCharacterStart = $trapText.IndexOf(
            $trapExpression,
            [StringComparison]::Ordinal
        )
        Assert-True -Condition ($trapCharacterStart -ge 0) `
            -Message "M26 trap fixture does not contain its expected expression"
        $trapPrefix = $trapText.Substring(0, $trapCharacterStart)
        $trapLine = [regex]::Matches($trapPrefix, "`r`n|`r|`n").Count + 1
        $trapLineStart = [Math]::Max(
            $trapPrefix.LastIndexOf("`r"),
            $trapPrefix.LastIndexOf("`n")
        ) + 1
        $trapColumn = $trapPrefix.Length - $trapLineStart + 1
        $trapByteStart = $utf8NoBom.GetByteCount($trapPrefix)
        $trapByteEnd = $trapByteStart + $utf8NoBom.GetByteCount($trapExpression)
        $expectedTrapDiagnostic = "arche: trap[I32_DIVIDE_BY_ZERO] m26_trap.arc:${trapLine}:${trapColumn} bytes ${trapByteStart}..${trapByteEnd}`n"
        Assert-Equal -Name "M26 exact trap diagnostic" `
            -Actual $trapDiagnostic -Expected $expectedTrapDiagnostic
        Assert-Contains -Name "M26 trap committed current value" `
            -Actual $trapRun.Result.Stdout -Expected " 4 2A000000"
    }

    Test-MetadataRejections -Artifact $closureArtifact `
        -Layout $closureLayout -Package $closurePackage

    $testList = Invoke-CapturedProcess -Name "enumerate registered M26 proofs" `
        -Executable "cargo" `
        -Arguments @("test", "--locked", "--bin", "archec0", "--manifest-path", $manifestPath, "--", "--list")
    Assert-ProcessStatus $testList 0
    Assert-ProofTestInventory $testList

    $powerShellExecutable = (Get-Process -Id $PID).Path
    $e2eRoot = Join-Path $repoRoot "tests/e2e"
    $e2eScripts = @(Get-ChildItem -LiteralPath $e2eRoot -Filter "*.ps1" -File |
        Where-Object { !$_.Name.StartsWith("_", [StringComparison]::Ordinal) } |
        Sort-Object Name)
    Assert-True -Condition ($e2eScripts.Count -gt 0) `
        -Message "no executable e2e PowerShell scripts were discovered"
    foreach ($script in $e2eScripts) {
        $scriptName = $script.Name
        $scriptPath = $script.FullName
        $arguments = @(
            "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $scriptPath,
            "-CompilerPath", $compiler,
            "-BuildDirectory", (Join-Path $proofRoot ([IO.Path]::GetFileNameWithoutExtension($scriptName)))
        )
        if ($SkipGeneratedLinuxExecution) {
            $arguments += "-SkipGeneratedLinuxExecution"
        }
        $e2e = Invoke-CapturedProcess -Name "e2e $scriptName" `
            -Executable $powerShellExecutable -Arguments $arguments
        Assert-ProcessStatus $e2e 0
    }
    Write-Host "PASS: dynamically discovered $($e2eScripts.Count) e2e scripts"

    Test-NoSiblingTemporaries $proofRoot
    Write-Host "All M26 and M27-B proof checks passed"
}
finally {
    Pop-Location
    if (Test-Path -LiteralPath $proofRoot) {
        Remove-Item -LiteralPath $proofRoot -Recurse -Force
    }
}
