param(
    [switch] $SkipGeneratedLinuxExecution
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent (Split-Path -Parent $scriptDir)
$outputPath = Join-Path $repoRoot "build/e2e/arena_recovery"
$outputDir = Split-Path -Parent $outputPath
$isWindowsPlatform = [System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT
$isLinuxPlatform = $false
if (!$isWindowsPlatform) {
    $isLinuxVariable = Get-Variable -Name IsLinux -ErrorAction SilentlyContinue
    $isLinuxPlatform = $null -ne $isLinuxVariable -and [bool] $isLinuxVariable.Value
}

function Invoke-Archec0 {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    Write-Host "==> $Name"
    & cargo run --quiet --locked --manifest-path "./bootstrap/archec0/Cargo.toml" -- @Arguments
    if ($LASTEXITCODE -ne 0) {
        Write-Error "$Name failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }
    Write-Host "PASS: $Name"
}

function Invoke-Archec0WithOutput {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    Write-Host "==> $Name"
    $output = @(& cargo run --quiet --locked --manifest-path "./bootstrap/archec0/Cargo.toml" -- @Arguments)
    if ($LASTEXITCODE -ne 0) {
        Write-Error "$Name failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }
    Write-Host "PASS: $Name"
    return $output
}

function Assert-Equal {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [UInt64] $Actual,

        [Parameter(Mandatory = $true)]
        [UInt64] $Expected
    )

    if ($Actual -ne $Expected) {
        Write-Error "$Name expected $Expected but got $Actual"
        exit 1
    }
}

function Add-ByteSequence {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [System.Collections.Generic.List[byte]] $Bytes,

        [Parameter(Mandatory = $true)]
        [byte[]] $Sequence
    )

    foreach ($byte in $Sequence) {
        [void] $Bytes.Add($byte)
    }
}

function Add-StackFrameAdjust {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [System.Collections.Generic.List[byte]] $Bytes,

        [Parameter(Mandatory = $true)]
        [byte] $Opcode,

        [Parameter(Mandatory = $true)]
        [int] $FrameSize
    )

    if ($FrameSize -le 127) {
        Add-ByteSequence -Bytes $Bytes -Sequence ([byte[]]@(0x48, 0x83, $Opcode, [byte]$FrameSize))
        return
    }

    Add-ByteSequence -Bytes $Bytes -Sequence ([byte[]]@(0x48, 0x81, $Opcode))
    Add-ByteSequence -Bytes $Bytes -Sequence ([BitConverter]::GetBytes([UInt32]$FrameSize))
}

function Add-ZeroQwordStore {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [System.Collections.Generic.List[byte]] $Bytes,

        [Parameter(Mandatory = $true)]
        [int] $Offset
    )

    if ($Offset -eq 0) {
        Add-ByteSequence -Bytes $Bytes -Sequence ([byte[]]@(0x48, 0x89, 0x04, 0x24))
    } elseif ($Offset -le 127) {
        Add-ByteSequence -Bytes $Bytes -Sequence ([byte[]]@(0x48, 0x89, 0x44, 0x24, [byte]$Offset))
    } else {
        Add-ByteSequence -Bytes $Bytes -Sequence ([byte[]]@(0x48, 0x89, 0x84, 0x24))
        Add-ByteSequence -Bytes $Bytes -Sequence ([BitConverter]::GetBytes([UInt32]$Offset))
    }
}

function New-RuntimeStateQwordOffsets {
    param(
        [Parameter(Mandatory = $true)]
        [int] $FrameSize
    )

    0..(($FrameSize / 8) - 1) | ForEach-Object { $_ * 8 }
}

function New-RuntimeCreatePrefix {
    param(
        [Parameter(Mandatory = $true)]
        [int] $FrameSize
    )

    $bytes = [System.Collections.Generic.List[byte]]::new()
    Add-StackFrameAdjust -Bytes $bytes -Opcode 0xec -FrameSize $FrameSize
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0x31, 0xc0))
    foreach ($offset in (New-RuntimeStateQwordOffsets -FrameSize $FrameSize)) {
        Add-ZeroQwordStore -Bytes $bytes -Offset $offset
    }
    return [byte[]]$bytes.ToArray()
}

function New-RuntimeDestroySuffix {
    param(
        [Parameter(Mandatory = $true)]
        [int] $FrameSize
    )

    $bytes = [System.Collections.Generic.List[byte]]::new()
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0x31, 0xc0))
    foreach ($offset in (New-RuntimeStateQwordOffsets -FrameSize $FrameSize)) {
        Add-ZeroQwordStore -Bytes $bytes -Offset $offset
    }
    Add-StackFrameAdjust -Bytes $bytes -Opcode 0xc4 -FrameSize $FrameSize
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05))
    return [byte[]]$bytes.ToArray()
}

function Get-RuntimeFrameSizeAtEntry {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [int] $EntryFileOffset
    )

    if (($EntryFileOffset -lt 0) -or ($EntryFileOffset + 4 -gt $Bytes.Length)) {
        Write-Error "runtime entry does not contain a complete stack-frame instruction"
        exit 1
    }
    if (($Bytes[$EntryFileOffset] -ne 0x48) -or ($Bytes[$EntryFileOffset + 2] -ne 0xec)) {
        Write-Error "runtime entry does not begin with sub rsp"
        exit 1
    }

    if ($Bytes[$EntryFileOffset + 1] -eq 0x83) {
        $frameSize = [int]$Bytes[$EntryFileOffset + 3]
    } elseif ($Bytes[$EntryFileOffset + 1] -eq 0x81) {
        if ($EntryFileOffset + 7 -gt $Bytes.Length) {
            Write-Error "runtime entry does not contain the complete sub rsp immediate"
            exit 1
        }
        $rawFrameSize = [BitConverter]::ToUInt32($Bytes, $EntryFileOffset + 3)
        if ($rawFrameSize -gt [int]::MaxValue) {
            Write-Error "runtime frame size exceeds the supported signed range"
            exit 1
        }
        $frameSize = [int]$rawFrameSize
    } else {
        Write-Error "runtime entry uses an unsupported sub rsp encoding"
        exit 1
    }

    if (($frameSize -le 0) -or (($frameSize % 16) -ne 0)) {
        Write-Error "runtime frame size must be positive and 16-byte aligned, got $frameSize"
        exit 1
    }
    return $frameSize
}

function Find-LastByteSequence {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [byte[]] $Sequence,

        [Parameter(Mandatory = $true)]
        [int] $Start
    )

    $last = -1
    for ($offset = $Start; $offset -le $Bytes.Length - $Sequence.Length; $offset++) {
        $matches = $true
        for ($index = 0; $index -lt $Sequence.Length; $index++) {
            if ($Bytes[$offset + $index] -ne $Sequence[$index]) {
                $matches = $false
                break
            }
        }
        if ($matches) {
            $last = $offset
        }
    }
    return $last
}

function Assert-ByteRange {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [int] $Offset,

        [Parameter(Mandatory = $true)]
        [byte[]] $Expected,

        [Parameter(Mandatory = $true)]
        [string] $Name
    )

    if (($Offset -lt 0) -or ($Offset + $Expected.Length -gt $Bytes.Length)) {
        Write-Error "$Name is outside the artifact"
        exit 1
    }
    for ($index = 0; $index -lt $Expected.Length; $index++) {
        if ($Bytes[$Offset + $index] -ne $Expected[$index]) {
            Write-Error "$Name byte $index does not match"
            exit 1
        }
    }
}

function Assert-PublishedRuntimeEnvelope {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    if (!(Test-Path -LiteralPath $Path -PathType Leaf)) {
        Write-Error "published Arena ELF was not created: $Path"
        exit 1
    }
    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
    if ($bytes.Length -le 120) {
        Write-Error "published Arena ELF is too small"
        exit 1
    }
    Assert-ByteRange -Bytes $bytes -Offset 0 -Expected ([byte[]]@(0x7f, 0x45, 0x4c, 0x46)) -Name "ELF magic"

    $loadOffset = [BitConverter]::ToUInt64($bytes, 72)
    $loadVaddr = [BitConverter]::ToUInt64($bytes, 80)
    $entrypoint = [BitConverter]::ToUInt64($bytes, 24)
    $entryFileOffset = $loadOffset + ($entrypoint - $loadVaddr)
    Assert-Equal -Name "ELF entry file offset" -Actual $entryFileOffset -Expected 120
    Assert-Equal -Name "ELF load file size" -Actual ([BitConverter]::ToUInt64($bytes, 96)) -Expected $bytes.Length
    Assert-Equal -Name "ELF load memory size" -Actual ([BitConverter]::ToUInt64($bytes, 104)) -Expected $bytes.Length

    $metadataMagic = [Text.Encoding]::ASCII.GetBytes("ARCHEECS")
    $metadataStart = Find-LastByteSequence -Bytes $bytes -Sequence $metadataMagic -Start ([int]$entryFileOffset)
    if ($metadataStart -le [int]$entryFileOffset) {
        Write-Error "published Arena ELF does not contain trailing ARCHEECS metadata"
        exit 1
    }

    $frameSize = Get-RuntimeFrameSizeAtEntry -Bytes $bytes -EntryFileOffset ([int]$entryFileOffset)
    [byte[]]$expectedPrefix = New-RuntimeCreatePrefix -FrameSize $frameSize
    [byte[]]$expectedSuffix = New-RuntimeDestroySuffix -FrameSize $frameSize
    $suffixStart = $metadataStart - $expectedSuffix.Length
    if ([int]$entryFileOffset + $expectedPrefix.Length -gt $suffixStart) {
        Write-Error "Arena runtime text is too short for its discovered frame envelope"
        exit 1
    }
    Assert-ByteRange -Bytes $bytes -Offset ([int]$entryFileOffset) -Expected $expectedPrefix -Name "runtime create prefix"
    Assert-ByteRange -Bytes $bytes -Offset $suffixStart -Expected $expectedSuffix -Name "runtime destroy suffix"
    Write-Host "PASS: published Arena ELF with runtime frame envelope ($frameSize bytes)"
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

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue

Push-Location $repoRoot
try {
    Invoke-Archec0 -Name "Arena semantic check" -Arguments @("./examples/arena_recovery.arc", "--check")

    $firstCore = @(Invoke-Archec0WithOutput -Name "Arena first Core emission" -Arguments @("./examples/arena_recovery.arc", "--emit-core"))
    $secondCore = @(Invoke-Archec0WithOutput -Name "Arena second Core emission" -Arguments @("./examples/arena_recovery.arc", "--emit-core"))
    if (($firstCore.Count -ne $secondCore.Count) -or ($firstCore.Count -eq 0)) {
        Write-Error "Arena Core emission is empty or nondeterministic"
        exit 1
    }
    for ($index = 0; $index -lt $firstCore.Count; $index++) {
        if ($firstCore[$index] -cne $secondCore[$index]) {
            Write-Error "Arena Core emission differs at line $($index + 1)"
            exit 1
        }
    }
    if (($firstCore -notcontains "world Arena") -or ($firstCore -notcontains "system Recover {")) {
        Write-Error "Arena Core emission does not identify the unrelated acceptance program"
        exit 1
    }
    Write-Host "PASS: deterministic Arena Core emission"

    Invoke-Archec0 -Name "Arena publication" -Arguments @("./examples/arena_recovery.arc", "-o", "./build/e2e/arena_recovery")
    Assert-PublishedRuntimeEnvelope -Path $outputPath

    if ($SkipGeneratedLinuxExecution) {
        Write-Host "SKIP: arena_recovery generated Linux execution (-SkipGeneratedLinuxExecution)"
    } else {
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

        Assert-Equal -Name "Arena generated Linux exit code" -Actual $LASTEXITCODE -Expected 47
        Write-Host "PASS: arena_recovery exits 47"
    }
}
finally {
    Pop-Location
    Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
}
