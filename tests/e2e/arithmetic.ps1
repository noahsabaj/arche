param(
    [switch] $SkipGeneratedLinuxExecution
)

$ErrorActionPreference = "Stop"
$DefaultNativeRuntimeFrameSize = 1088

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent (Split-Path -Parent $scriptDir)
$outputDir = Join-Path $repoRoot "build/e2e"
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
        [int] $FrameSize = $DefaultNativeRuntimeFrameSize
    )

    0..(($FrameSize / 8) - 1) | ForEach-Object { $_ * 8 }
}

function New-RuntimeCreatePrefix {
    param(
        [int] $FrameSize = $DefaultNativeRuntimeFrameSize
    )

    $bytes = [System.Collections.Generic.List[byte]]::new()
    Add-StackFrameAdjust -Bytes $bytes -Opcode 0xec -FrameSize $FrameSize
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0x31, 0xc0))
    foreach ($offset in (New-RuntimeStateQwordOffsets -FrameSize $FrameSize)) {
        Add-ZeroQwordStore -Bytes $bytes -Offset $offset
    }
    [byte[]]$bytes.ToArray()
}

function New-RuntimeDestroySuffix {
    param(
        [int] $FrameSize = $DefaultNativeRuntimeFrameSize
    )

    $bytes = [System.Collections.Generic.List[byte]]::new()
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0x31, 0xc0))
    foreach ($offset in (New-RuntimeStateQwordOffsets -FrameSize $FrameSize)) {
        Add-ZeroQwordStore -Bytes $bytes -Offset $offset
    }
    Add-StackFrameAdjust -Bytes $bytes -Opcode 0xc4 -FrameSize $FrameSize
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05))
    [byte[]]$bytes.ToArray()
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

function Assert-RuntimeFrameEnvelope {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [int] $EntryFileOffset,

        [Parameter(Mandatory = $true)]
        [UInt64] $TextEnd
    )

    if (($TextEnd -gt [UInt64]$Bytes.Length) -or ($TextEnd -gt [int]::MaxValue)) {
        Write-Error "runtime text end is outside the artifact"
        exit 1
    }

    $frameSize = Get-RuntimeFrameSizeAtEntry -Bytes $Bytes -EntryFileOffset $EntryFileOffset
    [byte[]]$expectedPrefix = New-RuntimeCreatePrefix -FrameSize $frameSize
    [byte[]]$expectedSuffix = New-RuntimeDestroySuffix -FrameSize $frameSize
    $textEndInt = [int]$TextEnd
    $suffixStart = $textEndInt - $expectedSuffix.Length

    if (($EntryFileOffset + $expectedPrefix.Length -gt $suffixStart) -or ($suffixStart -lt 0)) {
        Write-Error "runtime text is too short for its discovered frame envelope"
        exit 1
    }

    for ($i = 0; $i -lt $expectedPrefix.Length; $i++) {
        if ($Bytes[$EntryFileOffset + $i] -ne $expectedPrefix[$i]) {
            Write-Error "runtime create prefix byte $i does not match discovered frame size $frameSize"
            exit 1
        }
    }
    for ($i = 0; $i -lt $expectedSuffix.Length; $i++) {
        if ($Bytes[$suffixStart + $i] -ne $expectedSuffix[$i]) {
            Write-Error "runtime destroy suffix byte $i does not match discovered frame size $frameSize"
            exit 1
        }
    }

    Write-Host "PASS: runtime frame envelope ($frameSize bytes)"
}

function New-RuntimeWrappedText {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $StartupBody
    )

    $bytes = New-Object System.Collections.Generic.List[byte]

    foreach ($byte in [byte[]](New-RuntimeCreatePrefix)) {
        [void] $bytes.Add($byte)
    }

    foreach ($byte in $StartupBody) {
        [void] $bytes.Add($byte)
    }

    foreach ($byte in [byte[]](New-RuntimeDestroySuffix)) {
        [void] $bytes.Add($byte)
    }

    return [byte[]] $bytes.ToArray()
}

function New-AddRuntimeText {
    param(
        [Parameter(Mandatory = $true)]
        [UInt32] $Left,

        [Parameter(Mandatory = $true)]
        [UInt32] $Right
    )

    $leftImmediate = [BitConverter]::GetBytes($Left)
    $rightImmediate = [BitConverter]::GetBytes($Right)
    $startupBody = [byte[]]@(
        0x48, 0x83, 0xec, 0x08,
        0xc7, 0x04, 0x24, $leftImmediate[0], $leftImmediate[1], $leftImmediate[2], $leftImmediate[3],
        0x81, 0x04, 0x24, $rightImmediate[0], $rightImmediate[1], $rightImmediate[2], $rightImmediate[3],
        0x8b, 0x3c, 0x24,
        0x48, 0x83, 0xc4, 0x08
    )

    return New-RuntimeWrappedText -StartupBody $startupBody
}

function New-SubRuntimeText {
    param(
        [Parameter(Mandatory = $true)]
        [UInt32] $Left,

        [Parameter(Mandatory = $true)]
        [UInt32] $Right
    )

    $leftImmediate = [BitConverter]::GetBytes($Left)
    $rightImmediate = [BitConverter]::GetBytes($Right)
    $startupBody = [byte[]]@(
        0x48, 0x83, 0xec, 0x08,
        0xc7, 0x04, 0x24, $leftImmediate[0], $leftImmediate[1], $leftImmediate[2], $leftImmediate[3],
        0x81, 0x2c, 0x24, $rightImmediate[0], $rightImmediate[1], $rightImmediate[2], $rightImmediate[3],
        0x8b, 0x3c, 0x24,
        0x48, 0x83, 0xc4, 0x08
    )

    return New-RuntimeWrappedText -StartupBody $startupBody
}

function New-MulRuntimeText {
    param(
        [Parameter(Mandatory = $true)]
        [UInt32] $Left,

        [Parameter(Mandatory = $true)]
        [UInt32] $Right
    )

    $leftImmediate = [BitConverter]::GetBytes($Left)
    $rightImmediate = [BitConverter]::GetBytes($Right)
    $startupBody = [byte[]]@(
        0x48, 0x83, 0xec, 0x08,
        0xc7, 0x04, 0x24, $leftImmediate[0], $leftImmediate[1], $leftImmediate[2], $leftImmediate[3],
        0x69, 0x3c, 0x24, $rightImmediate[0], $rightImmediate[1], $rightImmediate[2], $rightImmediate[3],
        0x48, 0x83, 0xc4, 0x08
    )

    return New-RuntimeWrappedText -StartupBody $startupBody
}

function Test-Payload {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [byte[]] $ExpectedText
    )

    if (!(Test-Path -LiteralPath $Path)) {
        Write-Error "ELF output not found: $Path"
        exit 1
    }

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
    Assert-Equal -Name "ELF text payload length" -Actual ($bytes.Length - 120) -Expected $ExpectedText.Length
    Assert-RuntimeFrameEnvelope -Bytes $bytes -EntryFileOffset 120 -TextEnd ([UInt64]$bytes.Length)

    for ($i = 0; $i -lt $ExpectedText.Length; $i++) {
        Assert-Equal -Name "ELF text payload byte $i" -Actual $bytes[120 + $i] -Expected $ExpectedText[$i]
    }
}

function Test-ArithmeticCase {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string] $Source,

        [Parameter(Mandatory = $true)]
        [string] $Output,

        [Parameter(Mandatory = $true)]
        [byte[]] $ExpectedText,

        [Parameter(Mandatory = $true)]
        [int] $ExpectedExitCode
    )

    $outputPath = Join-Path $repoRoot $Output
    Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue

    & cargo run --locked --manifest-path "./bootstrap/archec0/Cargo.toml" -- $Source "-o" $Output
    if ($LASTEXITCODE -ne 0) {
        Write-Error "$Name build failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    Test-Payload -Path $outputPath -ExpectedText $ExpectedText

    if ($SkipGeneratedLinuxExecution) {
        Write-Host "SKIP: $Name generated Linux execution (-SkipGeneratedLinuxExecution)"
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

    Assert-Equal -Name "$Name exit code" -Actual $LASTEXITCODE -Expected $ExpectedExitCode

    Write-Host "PASS: $Name exits $ExpectedExitCode"
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

$addText = New-AddRuntimeText -Left 40 -Right 2
$subText = New-SubRuntimeText -Left 50 -Right 8
$mulText = New-MulRuntimeText -Left 6 -Right 7

Push-Location $repoRoot
try {
    Test-ArithmeticCase -Name "add math" -Source "./examples/math.arc" -Output "./build/e2e/math" -ExpectedText $addText -ExpectedExitCode 42
    Test-ArithmeticCase -Name "sub42" -Source "./examples/sub42.arc" -Output "./build/e2e/sub42" -ExpectedText $subText -ExpectedExitCode 42
    Test-ArithmeticCase -Name "mul42" -Source "./examples/mul42.arc" -Output "./build/e2e/mul42" -ExpectedText $mulText -ExpectedExitCode 42
}
finally {
    Pop-Location
}
