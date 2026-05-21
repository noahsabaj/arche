$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent (Split-Path -Parent $scriptDir)
$outputDir = Join-Path $repoRoot "build\e2e"

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
    0..106 | ForEach-Object { $_ * 8 }
}

function New-RuntimeCreatePrefix {
    $bytes = [System.Collections.Generic.List[byte]]::new()
    Add-StackFrameAdjust -Bytes $bytes -Opcode 0xec -FrameSize 856
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0x31, 0xc0))
    foreach ($offset in (New-RuntimeStateQwordOffsets)) {
        Add-ZeroQwordStore -Bytes $bytes -Offset $offset
    }
    [byte[]]$bytes.ToArray()
}

function New-RuntimeDestroySuffix {
    $bytes = [System.Collections.Generic.List[byte]]::new()
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0x31, 0xc0))
    foreach ($offset in (New-RuntimeStateQwordOffsets)) {
        Add-ZeroQwordStore -Bytes $bytes -Offset $offset
    }
    Add-StackFrameAdjust -Bytes $bytes -Opcode 0xc4 -FrameSize 856
    Add-ByteSequence -Bytes $bytes -Sequence ([byte[]]@(0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05))
    [byte[]]$bytes.ToArray()
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

    & cargo run --manifest-path ".\bootstrap\archec0\Cargo.toml" -- $Source "-o" $Output
    if ($LASTEXITCODE -ne 0) {
        Write-Error "$Name build failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    Test-Payload -Path $outputPath -ExpectedText $ExpectedText

    $wslPath = ConvertTo-WslPath -Path $outputPath
    & wsl $wslPath
    Assert-Equal -Name "$Name exit code" -Actual $LASTEXITCODE -Expected $ExpectedExitCode

    Write-Host "PASS: $Name exits $ExpectedExitCode"
}

if (!(Get-Command wsl -ErrorAction SilentlyContinue)) {
    Write-Error "wsl.exe is required to run the generated Linux ELF"
    exit 1
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

$addText = New-AddRuntimeText -Left 40 -Right 2
$subText = New-SubRuntimeText -Left 50 -Right 8
$mulText = New-MulRuntimeText -Left 6 -Right 7

Push-Location $repoRoot
try {
    Test-ArithmeticCase -Name "add math" -Source ".\examples\math.arc" -Output ".\build\e2e\math" -ExpectedText $addText -ExpectedExitCode 42
    Test-ArithmeticCase -Name "sub42" -Source ".\examples\sub42.arc" -Output ".\build\e2e\sub42" -ExpectedText $subText -ExpectedExitCode 42
    Test-ArithmeticCase -Name "mul42" -Source ".\examples\mul42.arc" -Output ".\build\e2e\mul42" -ExpectedText $mulText -ExpectedExitCode 42
}
finally {
    Pop-Location
}
