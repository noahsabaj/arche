$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptDir
$e2eDir = Join-Path $repoRoot "tests\e2e"

function Invoke-CheckedCommand {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string] $Executable,

        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    Write-Host "==> $Name"
    & $Executable @Arguments

    if ($LASTEXITCODE -ne 0) {
        Write-Error "$Name failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    Write-Host "PASS: $Name"
}

function Invoke-CheckedCommandWithOutput {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string] $Executable,

        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    Write-Host "==> $Name"
    $output = @(& $Executable @Arguments)

    if ($LASTEXITCODE -ne 0) {
        Write-Error "$Name failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    Write-Host "PASS: $Name"
    return $output
}

function Invoke-CommandExpectFailure {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string] $Executable,

        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    Write-Host "==> $Name"
    $previousErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = @(& $Executable @Arguments 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }

    if ($exitCode -eq 0) {
        Write-Error "$Name was expected to fail but exited 0"
        exit 1
    }

    Write-Host "PASS: $Name failed as expected"
    return @($output | ForEach-Object { $_.ToString() })
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

function Assert-StringEqual {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string] $Actual,

        [Parameter(Mandatory = $true)]
        [string] $Expected
    )

    if ($Actual -ne $Expected) {
        Write-Error "$Name expected '$Expected' but got '$Actual'"
        exit 1
    }
}

function ConvertFrom-HexUInt64 {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Hex
    )

    return [UInt64]::Parse(
        $Hex,
        [System.Globalization.NumberStyles]::HexNumber,
        [System.Globalization.CultureInfo]::InvariantCulture
    )
}

function Assert-OutputContains {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [string[]] $Output,

        [Parameter(Mandatory = $true)]
        [string] $ExpectedText
    )

    $text = $Output -join "`n"
    if (!$text.Contains($ExpectedText)) {
        Write-Error "$Name expected output to contain '$ExpectedText'"
        exit 1
    }
}

function Assert-BytesEqual {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [byte[]] $Actual,

        [Parameter(Mandatory = $true)]
        [byte[]] $Expected
    )

    if ($Actual.Length -ne $Expected.Length) {
        Write-Error "$Name expected $($Expected.Length) bytes but got $($Actual.Length)"
        exit 1
    }

    for ($i = 0; $i -lt $Expected.Length; $i++) {
        if ($Actual[$i] -ne $Expected[$i]) {
            Write-Error "$Name byte $i expected $($Expected[$i]) but got $($Actual[$i])"
            exit 1
        }
    }
}

function Assert-LinesEqual {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string[]] $Actual,

        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string[]] $Expected
    )

    if ($Actual.Count -ne $Expected.Count) {
        Write-Error "$Name expected $($Expected.Count) lines but got $($Actual.Count)"
        exit 1
    }

    for ($i = 0; $i -lt $Expected.Count; $i++) {
        if ($Actual[$i] -ne $Expected[$i]) {
            Write-Error "$Name line $($i + 1) expected '$($Expected[$i])' but got '$($Actual[$i])'"
            exit 1
        }
    }
}

function Test-Elf64Payload {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [byte[]] $ExpectedText,

        [Parameter(Mandatory = $false)]
        [UInt64] $ExpectedTrailingPayloadLength = 0
    )

    Write-Host "==> ELF64 header check"

    if (!(Test-Path -LiteralPath $Path)) {
        Write-Error "ELF output not found: $Path"
        exit 1
    }

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))

    if ($bytes.Length -lt 120) {
        Write-Error "ELF output is too small: $($bytes.Length) bytes"
        exit 1
    }

    Assert-Equal -Name "ELF magic byte 0" -Actual $bytes[0] -Expected 0x7f
    Assert-Equal -Name "ELF magic byte 1" -Actual $bytes[1] -Expected 0x45
    Assert-Equal -Name "ELF magic byte 2" -Actual $bytes[2] -Expected 0x4c
    Assert-Equal -Name "ELF magic byte 3" -Actual $bytes[3] -Expected 0x46
    Assert-Equal -Name "ELF class" -Actual $bytes[4] -Expected 2
    Assert-Equal -Name "ELF data encoding" -Actual $bytes[5] -Expected 1
    $entrypoint = [BitConverter]::ToUInt64($bytes, 24)
    $loadFlags = [BitConverter]::ToUInt32($bytes, 68)
    $loadOffset = [BitConverter]::ToUInt64($bytes, 72)
    $loadVaddr = [BitConverter]::ToUInt64($bytes, 80)
    $loadFileSize = [BitConverter]::ToUInt64($bytes, 96)

    Assert-Equal -Name "ELF type" -Actual ([BitConverter]::ToUInt16($bytes, 16)) -Expected 2
    Assert-Equal -Name "ELF machine" -Actual ([BitConverter]::ToUInt16($bytes, 18)) -Expected 0x3e
    Assert-Equal -Name "ELF program header entry size" -Actual ([BitConverter]::ToUInt16($bytes, 54)) -Expected 56
    Assert-Equal -Name "ELF load segment flags" -Actual $loadFlags -Expected 5

    $programHeaderCount = [BitConverter]::ToUInt16($bytes, 56)
    if ($programHeaderCount -lt 1) {
        Write-Error "ELF program header count expected at least 1 but got $programHeaderCount"
        exit 1
    }

    Write-Host "PASS: ELF64 header check"
    Write-Host "==> ELF text payload check"

    if ($bytes.Length -le 120) {
        Write-Error "ELF output does not contain text payload bytes"
        exit 1
    }

    Assert-Equal -Name "ELF load segment file size" -Actual ([BitConverter]::ToUInt64($bytes, 96)) -Expected $bytes.Length
    Assert-Equal -Name "ELF load segment memory size" -Actual ([BitConverter]::ToUInt64($bytes, 104)) -Expected $bytes.Length
    Assert-Equal -Name "ELF text plus trailing payload length" -Actual ($bytes.Length - 120) -Expected ([UInt64]($expectedText.Length + $ExpectedTrailingPayloadLength))

    for ($i = 0; $i -lt $expectedText.Length; $i++) {
        Assert-Equal -Name "ELF text payload byte $i" -Actual $bytes[120 + $i] -Expected $expectedText[$i]
    }

    Write-Host "PASS: ELF text payload check"
    Write-Host "==> ELF entrypoint check"

    $expectedEntrypoint = $loadVaddr + 120
    Assert-Equal -Name "ELF entrypoint" -Actual $entrypoint -Expected $expectedEntrypoint

    if (($entrypoint -lt $loadVaddr) -or ($entrypoint -ge ($loadVaddr + $loadFileSize))) {
        Write-Error "ELF entrypoint is outside the executable load segment"
        exit 1
    }

    $entryFileOffset = [UInt64]$loadOffset + ($entrypoint - $loadVaddr)
    Assert-Equal -Name "ELF entrypoint file offset" -Actual $entryFileOffset -Expected 120

    if ($entryFileOffset -ge [UInt64]$bytes.Length) {
        Write-Error "ELF entrypoint file offset is outside the file"
        exit 1
    }

    $entryFileOffsetInt = [int]$entryFileOffset
    for ($i = 0; $i -lt $expectedText.Length; $i++) {
        Assert-Equal -Name "ELF entrypoint byte $i" -Actual $bytes[$entryFileOffsetInt + $i] -Expected $expectedText[$i]
    }

    Write-Host "PASS: ELF entrypoint check"
}

function Test-Elf64TrailingPayload {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [UInt64] $ExpectedTrailingPayloadLength
    )

    Write-Host "==> ELF64 header check"

    if (!(Test-Path -LiteralPath $Path)) {
        Write-Error "ELF output not found: $Path"
        exit 1
    }

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))

    if ($bytes.Length -lt (120 + $ExpectedTrailingPayloadLength + 1)) {
        Write-Error "ELF output is too small for expected trailing payload: $($bytes.Length) bytes"
        exit 1
    }

    Assert-Equal -Name "ELF magic byte 0" -Actual $bytes[0] -Expected 0x7f
    Assert-Equal -Name "ELF magic byte 1" -Actual $bytes[1] -Expected 0x45
    Assert-Equal -Name "ELF magic byte 2" -Actual $bytes[2] -Expected 0x4c
    Assert-Equal -Name "ELF magic byte 3" -Actual $bytes[3] -Expected 0x46
    Assert-Equal -Name "ELF class" -Actual $bytes[4] -Expected 2
    Assert-Equal -Name "ELF data encoding" -Actual $bytes[5] -Expected 1
    Assert-Equal -Name "ELF type" -Actual ([BitConverter]::ToUInt16($bytes, 16)) -Expected 2
    Assert-Equal -Name "ELF machine" -Actual ([BitConverter]::ToUInt16($bytes, 18)) -Expected 0x3e
    Assert-Equal -Name "ELF program header entry size" -Actual ([BitConverter]::ToUInt16($bytes, 54)) -Expected 56

    $entrypoint = [BitConverter]::ToUInt64($bytes, 24)
    $loadFlags = [BitConverter]::ToUInt32($bytes, 68)
    $loadOffset = [BitConverter]::ToUInt64($bytes, 72)
    $loadVaddr = [BitConverter]::ToUInt64($bytes, 80)
    $loadFileSize = [BitConverter]::ToUInt64($bytes, 96)
    $metadataStart = [UInt64]($bytes.Length - $ExpectedTrailingPayloadLength)

    Assert-Equal -Name "ELF load segment flags" -Actual $loadFlags -Expected 5
    Assert-Equal -Name "ELF load segment file size" -Actual $loadFileSize -Expected $bytes.Length
    Assert-Equal -Name "ELF load segment memory size" -Actual ([BitConverter]::ToUInt64($bytes, 104)) -Expected $bytes.Length
    Assert-Equal -Name "ELF entrypoint" -Actual $entrypoint -Expected ($loadVaddr + 120)
    Assert-Equal -Name "ELF entrypoint file offset" -Actual ([UInt64]$loadOffset + ($entrypoint - $loadVaddr)) -Expected 120

    if ($metadataStart -le 120) {
        Write-Error "ELF text payload is missing before trailing metadata"
        exit 1
    }

    Write-Host "PASS: ELF64 header check"
}

function Read-U32 {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [ref] $Offset
    )

    $value = [BitConverter]::ToUInt32($Bytes, $Offset.Value)
    $Offset.Value += 4
    return $value
}

function Read-U64 {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [ref] $Offset
    )

    $value = [BitConverter]::ToUInt64($Bytes, $Offset.Value)
    $Offset.Value += 8
    return $value
}

function Read-MetadataString {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [ref] $Offset
    )

    $length = Read-U32 -Bytes $Bytes -Offset $Offset
    $value = [System.Text.Encoding]::ASCII.GetString($Bytes, $Offset.Value, [int]$length)
    $Offset.Value += [int]$length
    return $value
}

function Read-MetadataBytes {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [ref] $Offset
    )

    $length = Read-U32 -Bytes $Bytes -Offset $Offset
    $value = New-Object byte[] $length
    [Array]::Copy($Bytes, $Offset.Value, $value, 0, $length)
    $Offset.Value += [int]$length
    return [byte[]]$value
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
    0..94 | ForEach-Object { $_ * 8 }
}

function New-RuntimeCreatePrefix {
    $bytes = [System.Collections.Generic.List[byte]]::new()
    Add-StackFrameAdjust -Bytes $bytes -Opcode 0xec -FrameSize 760
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
    Add-StackFrameAdjust -Bytes $bytes -Opcode 0xc4 -FrameSize 760
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

function New-ImmediateRuntimeText {
    param(
        [Parameter(Mandatory = $true)]
        [UInt64] $ExpectedExitCode
    )

    $exitImmediate = [BitConverter]::GetBytes([UInt32]$ExpectedExitCode)
    $startupBody = [byte[]]@(
        0xbf, $exitImmediate[0], $exitImmediate[1], $exitImmediate[2], $exitImmediate[3]
    )

    return New-RuntimeWrappedText -StartupBody $startupBody
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

function Test-PositionComponentMetadata {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $expectedText = New-ImmediateRuntimeText -ExpectedExitCode 0

    Test-Elf64Payload -Path $Path -ExpectedText $expectedText -ExpectedTrailingPayloadLength 85

    Write-Host "==> component metadata payload check"

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
    $offset = 120 + $expectedText.Length

    $magic = [System.Text.Encoding]::ASCII.GetString($bytes, $offset, 8)
    $offset += 8
    Assert-StringEqual -Name "component metadata magic" -Actual $magic -Expected "ARCHECMP"

    Assert-Equal -Name "component metadata version" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 1
    Assert-Equal -Name "component metadata count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 1
    Assert-Equal -Name "component metadata id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected 0x002202c6aeb4f27b
    Assert-StringEqual -Name "component metadata qualified name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Position"
    Assert-Equal -Name "component metadata size" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 8
    Assert-Equal -Name "component metadata align" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 4
    Assert-Equal -Name "component metadata field count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 2

    Assert-StringEqual -Name "component metadata field 0 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "x"
    Assert-StringEqual -Name "component metadata field 0 type" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "f32"
    Assert-Equal -Name "component metadata field 0 offset" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 0

    Assert-StringEqual -Name "component metadata field 1 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "y"
    Assert-StringEqual -Name "component metadata field 1 type" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "f32"
    Assert-Equal -Name "component metadata field 1 offset" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 4
    Assert-Equal -Name "component metadata end offset" -Actual $offset -Expected $bytes.Length

    Write-Host "PASS: component metadata payload check"
}

function Assert-EcsSection {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [int] $MetadataStart,

        [Parameter(Mandatory = $true)]
        [int] $Index,

        [Parameter(Mandatory = $true)]
        [UInt32] $ExpectedKind,

        [Parameter(Mandatory = $true)]
        [UInt32] $ExpectedOffset,

        [Parameter(Mandatory = $true)]
        [UInt32] $ExpectedByteLength,

        [Parameter(Mandatory = $true)]
        [UInt32] $ExpectedRecordCount
    )

    $offset = $MetadataStart + 16 + ($Index * 16)

    Assert-Equal -Name "ECS metadata section $Index kind" -Actual (Read-U32 -Bytes $Bytes -Offset ([ref]$offset)) -Expected $ExpectedKind
    Assert-Equal -Name "ECS metadata section $Index offset" -Actual (Read-U32 -Bytes $Bytes -Offset ([ref]$offset)) -Expected $ExpectedOffset
    Assert-Equal -Name "ECS metadata section $Index byte length" -Actual (Read-U32 -Bytes $Bytes -Offset ([ref]$offset)) -Expected $ExpectedByteLength
    Assert-Equal -Name "ECS metadata section $Index record count" -Actual (Read-U32 -Bytes $Bytes -Offset ([ref]$offset)) -Expected $ExpectedRecordCount
}

function Assert-EcsQueryTerms {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Name,

        [Parameter(Mandatory = $true)]
        [byte[]] $Bytes,

        [Parameter(Mandatory = $true)]
        [ref] $Offset
    )

    Assert-Equal -Name "$Name query term count" -Actual (Read-U32 -Bytes $Bytes -Offset $Offset) -Expected 2
    Assert-Equal -Name "$Name query term 0 access" -Actual (Read-U32 -Bytes $Bytes -Offset $Offset) -Expected 2
    Assert-Equal -Name "$Name query term 0 component id" -Actual (Read-U64 -Bytes $Bytes -Offset $Offset) -Expected (ConvertFrom-HexUInt64 "002202c6aeb4f27b")
    Assert-StringEqual -Name "$Name query term 0 component name" -Actual (Read-MetadataString -Bytes $Bytes -Offset $Offset) -Expected "Demo.Position"
    Assert-Equal -Name "$Name query term 1 access" -Actual (Read-U32 -Bytes $Bytes -Offset $Offset) -Expected 1
    Assert-Equal -Name "$Name query term 1 component id" -Actual (Read-U64 -Bytes $Bytes -Offset $Offset) -Expected (ConvertFrom-HexUInt64 "2cf8a68bcb7f913b")
    Assert-StringEqual -Name "$Name query term 1 component name" -Actual (Read-MetadataString -Bytes $Bytes -Offset $Offset) -Expected "Demo.Velocity"
}

function Test-EcsMetadataPayload {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    Test-Elf64TrailingPayload -Path $Path -ExpectedTrailingPayloadLength 717

    Write-Host "==> ECS metadata payload check"

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
    $metadataStart = $bytes.Length - 717
    $offset = $metadataStart

    $magic = [System.Text.Encoding]::ASCII.GetString($bytes, $offset, 8)
    $offset += 8
    Assert-StringEqual -Name "ECS metadata magic" -Actual $magic -Expected "ARCHEECS"
    Assert-Equal -Name "ECS metadata version" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 1
    Assert-Equal -Name "ECS metadata section count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 6

    Assert-EcsSection -Bytes $bytes -MetadataStart $metadataStart -Index 0 -ExpectedKind 1 -ExpectedOffset 112 -ExpectedByteLength 138 -ExpectedRecordCount 2
    Assert-EcsSection -Bytes $bytes -MetadataStart $metadataStart -Index 1 -ExpectedKind 2 -ExpectedOffset 250 -ExpectedByteLength 53 -ExpectedRecordCount 1
    Assert-EcsSection -Bytes $bytes -MetadataStart $metadataStart -Index 2 -ExpectedKind 3 -ExpectedOffset 303 -ExpectedByteLength 134 -ExpectedRecordCount 1
    Assert-EcsSection -Bytes $bytes -MetadataStart $metadataStart -Index 3 -ExpectedKind 4 -ExpectedOffset 437 -ExpectedByteLength 90 -ExpectedRecordCount 1
    Assert-EcsSection -Bytes $bytes -MetadataStart $metadataStart -Index 4 -ExpectedKind 5 -ExpectedOffset 527 -ExpectedByteLength 50 -ExpectedRecordCount 1
    Assert-EcsSection -Bytes $bytes -MetadataStart $metadataStart -Index 5 -ExpectedKind 6 -ExpectedOffset 577 -ExpectedByteLength 140 -ExpectedRecordCount 3

    $offset = $metadataStart + 112
    Assert-Equal -Name "ECS Position id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "002202c6aeb4f27b")
    Assert-StringEqual -Name "ECS Position name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Position"
    Assert-Equal -Name "ECS Position size" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 8
    Assert-Equal -Name "ECS Position align" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 4
    Assert-Equal -Name "ECS Position field count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 2
    Assert-StringEqual -Name "ECS Position field 0 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "x"
    Assert-StringEqual -Name "ECS Position field 0 type" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "f32"
    Assert-Equal -Name "ECS Position field 0 offset" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 0
    Assert-StringEqual -Name "ECS Position field 1 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "y"
    Assert-StringEqual -Name "ECS Position field 1 type" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "f32"
    Assert-Equal -Name "ECS Position field 1 offset" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 4

    Assert-Equal -Name "ECS Velocity id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "2cf8a68bcb7f913b")
    Assert-StringEqual -Name "ECS Velocity name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Velocity"
    Assert-Equal -Name "ECS Velocity size" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 8
    Assert-Equal -Name "ECS Velocity align" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 4
    Assert-Equal -Name "ECS Velocity field count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 2
    Assert-StringEqual -Name "ECS Velocity field 0 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "x"
    Assert-StringEqual -Name "ECS Velocity field 0 type" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "f32"
    Assert-Equal -Name "ECS Velocity field 0 offset" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 0
    Assert-StringEqual -Name "ECS Velocity field 1 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "y"
    Assert-StringEqual -Name "ECS Velocity field 1 type" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "f32"
    Assert-Equal -Name "ECS Velocity field 1 offset" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 4
    Assert-Equal -Name "ECS component section end" -Actual $offset -Expected ($metadataStart + 250)

    Assert-Equal -Name "ECS Time id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "7924ce11db524521")
    Assert-StringEqual -Name "ECS Time name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Time"
    Assert-Equal -Name "ECS Time size" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 4
    Assert-Equal -Name "ECS Time align" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 4
    Assert-Equal -Name "ECS Time field count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 1
    Assert-StringEqual -Name "ECS Time field 0 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "delta"
    Assert-StringEqual -Name "ECS Time field 0 type" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "f32"
    Assert-Equal -Name "ECS Time field 0 offset" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 0
    Assert-Equal -Name "ECS resource section end" -Actual $offset -Expected ($metadataStart + 303)

    Assert-Equal -Name "ECS Move id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "723b6b52df270ed5")
    Assert-StringEqual -Name "ECS Move name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Move"
    Assert-Equal -Name "ECS Move param count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 2
    Assert-StringEqual -Name "ECS Move param 0 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "time"
    Assert-Equal -Name "ECS Move param 0 kind" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 1
    Assert-Equal -Name "ECS Move param 0 resource id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "7924ce11db524521")
    Assert-StringEqual -Name "ECS Move param 0 resource name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Time"
    Assert-StringEqual -Name "ECS Move param 1 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "movers"
    Assert-Equal -Name "ECS Move param 1 kind" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 2
    Assert-EcsQueryTerms -Name "ECS Move param 1" -Bytes $bytes -Offset ([ref]$offset)
    Assert-Equal -Name "ECS system section end" -Actual $offset -Expected ($metadataStart + 437)

    Assert-Equal -Name "ECS movers query id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "f4004232b85cef9f")
    Assert-StringEqual -Name "ECS movers query name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Move.movers"
    Assert-EcsQueryTerms -Name "ECS movers" -Bytes $bytes -Offset ([ref]$offset)
    Assert-Equal -Name "ECS query section end" -Actual $offset -Expected ($metadataStart + 527)

    Assert-Equal -Name "ECS Main schedule id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "ed3d905325519b05")
    Assert-StringEqual -Name "ECS Main schedule name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Main"
    Assert-Equal -Name "ECS Main schedule item count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 1
    Assert-Equal -Name "ECS Main schedule item 0 kind" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 1
    Assert-Equal -Name "ECS Main schedule item 0 system id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "723b6b52df270ed5")
    Assert-StringEqual -Name "ECS Main schedule item 0 system name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Move"
    Assert-Equal -Name "ECS schedule section end" -Actual $offset -Expected ($metadataStart + 577)

    Assert-Equal -Name "ECS startup op 0 kind" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 1
    Assert-Equal -Name "ECS startup op 0 resource id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "7924ce11db524521")
    Assert-StringEqual -Name "ECS startup op 0 resource name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Time"
    Assert-BytesEqual -Name "ECS startup op 0 payload" -Actual (Read-MetadataBytes -Bytes $bytes -Offset ([ref]$offset)) -Expected ([byte[]]@(0x00, 0x00, 0x80, 0x3f))

    Assert-Equal -Name "ECS startup op 1 kind" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 2
    Assert-Equal -Name "ECS startup op 1 component count" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 2
    Assert-Equal -Name "ECS startup op 1 component 0 id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "002202c6aeb4f27b")
    Assert-StringEqual -Name "ECS startup op 1 component 0 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Position"
    Assert-BytesEqual -Name "ECS startup op 1 component 0 payload" -Actual (Read-MetadataBytes -Bytes $bytes -Offset ([ref]$offset)) -Expected ([byte[]]@(0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40))
    Assert-Equal -Name "ECS startup op 1 component 1 id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "2cf8a68bcb7f913b")
    Assert-StringEqual -Name "ECS startup op 1 component 1 name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Velocity"
    Assert-BytesEqual -Name "ECS startup op 1 component 1 payload" -Actual (Read-MetadataBytes -Bytes $bytes -Offset ([ref]$offset)) -Expected ([byte[]]@(0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40))

    Assert-Equal -Name "ECS startup op 2 kind" -Actual (Read-U32 -Bytes $bytes -Offset ([ref]$offset)) -Expected 3
    Assert-Equal -Name "ECS startup op 2 schedule id" -Actual (Read-U64 -Bytes $bytes -Offset ([ref]$offset)) -Expected (ConvertFrom-HexUInt64 "ed3d905325519b05")
    Assert-StringEqual -Name "ECS startup op 2 schedule name" -Actual (Read-MetadataString -Bytes $bytes -Offset ([ref]$offset)) -Expected "Demo.Main"
    Assert-Equal -Name "ECS metadata end offset" -Actual $offset -Expected $bytes.Length

    Write-Host "PASS: ECS metadata payload check"
}

function Test-CorruptEcsMetadataMagic {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_metadata"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $bytes[$metadataStart] = 0x58
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 16
}

function Test-CorruptEcsComponentDescriptorRecord {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_component_descriptor"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $descriptorSizeOffset = $metadataStart + 137
    Assert-Equal -Name "ECS Position descriptor size before corruption" -Actual $bytes[$descriptorSizeOffset] -Expected 0x08
    $bytes[$descriptorSizeOffset] = 0x0c
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsResourceDescriptorRecord {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_resource_descriptor"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $fieldOffsetOffset = $metadataStart + 299
    Assert-Equal -Name "ECS Time.delta field offset before corruption" -Actual $bytes[$fieldOffsetOffset] -Expected 0x00
    $bytes[$fieldOffsetOffset] = 0x04
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsSystemDescriptorRecord {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_system_descriptor"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $paramCountOffset = $metadataStart + 324
    Assert-Equal -Name "ECS Demo.Move param count before corruption" -Actual $bytes[$paramCountOffset] -Expected 0x02
    $bytes[$paramCountOffset] = 0x03
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsQueryDescriptorRecord {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_query_descriptor"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $termCountOffset = $metadataStart + 465
    Assert-Equal -Name "ECS Demo.Move.movers term count before corruption" -Actual $bytes[$termCountOffset] -Expected 0x02
    $bytes[$termCountOffset] = 0x03
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsScheduleDescriptorRecord {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_schedule_descriptor"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $scheduleItemKindOffset = $metadataStart + 552
    Assert-Equal -Name "ECS Demo.Main schedule item kind before corruption" -Actual $bytes[$scheduleItemKindOffset] -Expected 0x01
    $bytes[$scheduleItemKindOffset] = 0x09
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsStartupResourceId {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_startup_resource_id"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $resourceIdOffset = $metadataStart + 581
    Assert-Equal -Name "ECS startup resource id first byte before corruption" -Actual $bytes[$resourceIdOffset] -Expected 0x21
    $bytes[$resourceIdOffset] = 0x22
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsStartupSpawnComponentCount {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_startup_spawn_count"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $componentCountOffset = $metadataStart + 614
    Assert-Equal -Name "ECS startup spawn component count before corruption" -Actual $bytes[$componentCountOffset] -Expected 0x02
    $bytes[$componentCountOffset] = 0x03
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsStartupOperationKind {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_startup_operation_kind"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $operationKindOffset = $metadataStart + 577
    Assert-Equal -Name "ECS startup op 0 kind before corruption" -Actual $bytes[$operationKindOffset] -Expected 0x01
    $bytes[$operationKindOffset] = 0x09
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 21
}

function Test-CorruptEcsResourcePayload {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_resource_payload"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $payloadHighByteOffset = $metadataStart + 609
    Assert-Equal -Name "ECS resource payload high byte before corruption" -Actual $bytes[$payloadHighByteOffset] -Expected 0x3f
    $bytes[$payloadHighByteOffset] = 0x40
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsSpawnPayload {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_spawn_payload"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $payloadHighByteOffset = $metadataStart + 691
    Assert-Equal -Name "ECS spawn payload high byte before corruption" -Actual $bytes[$payloadHighByteOffset] -Expected 0x40
    $bytes[$payloadHighByteOffset] = 0x41
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 17
}

function Test-CorruptEcsRunSchedule {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $corruptPath = Join-Path (Split-Path -Parent $Path) "move_system_bad_run_schedule"
    Copy-Item -LiteralPath $Path -Destination $corruptPath -Force

    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $corruptPath))
    $metadataStart = $bytes.Length - 717
    $scheduleIdOffset = $metadataStart + 696
    Assert-Equal -Name "ECS startup run schedule id first byte before corruption" -Actual $bytes[$scheduleIdOffset] -Expected 0x05
    $bytes[$scheduleIdOffset] = 0x06
    [System.IO.File]::WriteAllBytes((Resolve-Path -LiteralPath $corruptPath), $bytes)

    Test-LinuxExitCode -Path $corruptPath -ExpectedExitCode 21
}

function Test-Elf64Executable {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [UInt64] $ExpectedExitCode
    )

    $expectedText = New-ImmediateRuntimeText -ExpectedExitCode $ExpectedExitCode

    Test-Elf64Payload -Path $Path -ExpectedText $expectedText
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

function Test-LinuxExitCode {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [UInt64] $ExpectedExitCode
    )

    Write-Host "==> Linux execution check"

    if (!(Get-Command wsl -ErrorAction SilentlyContinue)) {
        Write-Error "wsl.exe is required to run the generated Linux ELF"
        exit 1
    }

    $wslPath = ConvertTo-WslPath -Path $Path
    & wsl $wslPath
    $actualExitCode = $LASTEXITCODE

    Assert-Equal -Name "Linux executable exit code" -Actual $actualExitCode -Expected $ExpectedExitCode
    Write-Host "PASS: Linux execution check"
}

Push-Location $repoRoot
try {
    Invoke-CheckedCommand `
        -Name "core_represents_math_startup" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "core_represents_math_startup")

    Invoke-CheckedCommand `
        -Name "core_represents_move_system_body_model" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "core_represents_move_system_body_model")

    Invoke-CheckedCommand `
        -Name "lowers_query_loop_skeleton_to_core_body" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "lowers_query_loop_skeleton_to_core_body")

    Invoke-CheckedCommand `
        -Name "lowers_query_loop_field_expressions_to_core_body" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "lowers_query_loop_field_expressions_to_core_body")

    Invoke-CheckedCommand `
        -Name "lowers_query_loop_add_assign_to_core_body" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "lowers_query_loop_add_assign_to_core_body")

    Invoke-CheckedCommand `
        -Name "defines_native_move_query_loop_observable" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "defines_native_move_query_loop_observable")

    Invoke-CheckedCommand `
        -Name "defines_native_ecs_execution_state_layout" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "defines_native_ecs_execution_state_layout")

    Invoke-CheckedCommand `
        -Name "materializes_native_descriptor_record_state" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "materializes_native_descriptor_record_state")

    Invoke-CheckedCommand `
        -Name "decodes_native_component_resource_descriptor_records" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "decodes_native_component_resource_descriptor_records")

    Invoke-CheckedCommand `
        -Name "decodes_native_system_query_schedule_descriptor_records" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "decodes_native_system_query_schedule_descriptor_records")

    Invoke-CheckedCommand `
        -Name "dispatches_native_startup_operations" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "dispatches_native_startup_operations")

    Invoke-CheckedCommand `
        -Name "materializes_native_startup_operation_table" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "materializes_native_startup_operation_table")

    Invoke-CheckedCommand `
        -Name "materializes_native_query_planning_state" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "materializes_native_query_planning_state")

    Invoke-CheckedCommand `
        -Name "builds_native_query_plan_from_descriptor_records" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "builds_native_query_plan_from_descriptor_records")

    Invoke-CheckedCommand `
        -Name "executes_compiled_schedule_from_native_state" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "executes_compiled_schedule_from_native_state")

    Invoke-CheckedCommand `
        -Name "emits_native_query_loop_row_scan_skeleton" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "emits_native_query_loop_row_scan_skeleton")

    Invoke-CheckedCommand `
        -Name "emits_native_query_loop_field_multiply" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "emits_native_query_loop_field_multiply")

    Invoke-CheckedCommand `
        -Name "emits_native_query_loop_position_stores" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "emits_native_query_loop_position_stores")

    Invoke-CheckedCommand `
        -Name "replaces_bootstrap_move_helper_with_compiled_query_loop" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "replaces_bootstrap_move_helper_with_compiled_query_loop")

    Invoke-CheckedCommand `
        -Name "lowers_math_ast_to_core" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "lowers_math_ast_to_core")

    Invoke-CheckedCommand `
        -Name "lowers_spawn_position_to_core" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "lowers_spawn_position_to_core")

    Invoke-CheckedCommand `
        -Name "lowers_move_system_to_core_metadata" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "lowers_move_system_to_core_metadata")

    Invoke-CheckedCommand `
        -Name "lowers_schedule_to_core_metadata" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "lowers_schedule_to_core_metadata")

    Invoke-CheckedCommand `
        -Name "core_verifier_accepts_lowered_math" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "core_verifier_accepts_lowered_math")

    Invoke-CheckedCommand `
        -Name "core_verifier_rejects_invalid_value_reference" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "core_verifier_rejects_invalid_value_reference")

    Invoke-CheckedCommand `
        -Name "primitive_type_layouts" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "primitive_type_layouts")

    Invoke-CheckedCommand `
        -Name "computes_position_field_offsets" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "computes_position_field_offsets")

    Invoke-CheckedCommand `
        -Name "computes_position_component_layout" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "computes_position_component_layout")

    Invoke-CheckedCommand `
        -Name "stable_component_ids" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "stable_component_ids")

    Invoke-CheckedCommand `
        -Name "encodes_position_component_metadata" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "encodes_position_component_metadata")

    Invoke-CheckedCommand `
        -Name "defines_ecs_metadata_binary_envelope" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "defines_ecs_metadata_binary_envelope")

    Invoke-CheckedCommand `
        -Name "encodes_component_resource_descriptors_in_ecs_metadata" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "encodes_component_resource_descriptors_in_ecs_metadata")

    Invoke-CheckedCommand `
        -Name "encodes_system_query_schedule_descriptors_in_ecs_metadata" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "encodes_system_query_schedule_descriptors_in_ecs_metadata")

    Invoke-CheckedCommand `
        -Name "encodes_startup_operations_in_ecs_metadata" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "encodes_startup_operations_in_ecs_metadata")

    Invoke-CheckedCommand `
        -Name "defines_runtime_program_assembly_model" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "defines_runtime_program_assembly_model")

    Invoke-CheckedCommand `
        -Name "assembles_component_and_resource_descriptors_from_source" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "assembles_component_and_resource_descriptors_from_source")

    Invoke-CheckedCommand `
        -Name "assembles_system_query_and_schedule_descriptors_from_source" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "assembles_system_query_and_schedule_descriptors_from_source")

    Invoke-CheckedCommand `
        -Name "assembles_startup_resource_payload_operation" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "assembles_startup_resource_payload_operation")

    Invoke-CheckedCommand `
        -Name "assembles_startup_spawn_operation" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "assembles_startup_spawn_operation")

    Invoke-CheckedCommand `
        -Name "assembles_startup_run_operation" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "assembles_startup_run_operation")

    Invoke-CheckedCommand `
        -Name "registers_assembly_descriptors_into_world" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "registers_assembly_descriptors_into_world")

    Invoke-CheckedCommand `
        -Name "executes_startup_resource_payload_operation" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "executes_startup_resource_payload_operation")

    Invoke-CheckedCommand `
        -Name "executes_startup_spawn_operation" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "executes_startup_spawn_operation")

    Invoke-CheckedCommand `
        -Name "executes_startup_run_schedule_operation" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "executes_startup_run_schedule_operation")

    Invoke-CheckedCommand `
        -Name "executes_move_system_source_runtime_vertical_slice" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "executes_move_system_source_runtime_vertical_slice")

    Invoke-CheckedCommand `
        -Name "arche_entity_packs_index_and_generation" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "arche_entity_packs_index_and_generation")

    Invoke-CheckedCommand `
        -Name "entity_table_allocates_and_reuses_generation" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "entity_table_allocates_and_reuses_generation")

    Invoke-CheckedCommand `
        -Name "registers_position_component_descriptor" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "registers_position_component_descriptor")

    Invoke-CheckedCommand `
        -Name "defines_time_delta_resource_descriptor" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "defines_time_delta_resource_descriptor")

    Invoke-CheckedCommand `
        -Name "registers_move_system_descriptor" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "registers_move_system_descriptor")

    Invoke-CheckedCommand `
        -Name "registers_main_schedule_descriptor" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "registers_main_schedule_descriptor")

    Invoke-CheckedCommand `
        -Name "builds_sequential_schedule_plan" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "builds_sequential_schedule_plan")

    Invoke-CheckedCommand `
        -Name "executes_runtime_schedule_plan" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "executes_runtime_schedule_plan")

    Invoke-CheckedCommand `
        -Name "defines_position_velocity_query_descriptor" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "defines_position_velocity_query_descriptor")

    Invoke-CheckedCommand `
        -Name "matches_position_velocity_query_to_archetype" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "matches_position_velocity_query_to_archetype")

    Invoke-CheckedCommand `
        -Name "builds_position_velocity_query_plan" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "builds_position_velocity_query_plan")

    Invoke-CheckedCommand `
        -Name "iterates_position_velocity_query_rows" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "iterates_position_velocity_query_rows")

    Invoke-CheckedCommand `
        -Name "reads_time_delta_during_query_iteration" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "reads_time_delta_during_query_iteration")

    Invoke-CheckedCommand `
        -Name "applies_move_system_to_position_rows" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "applies_move_system_to_position_rows")

    Invoke-CheckedCommand `
        -Name "allocates_time_delta_resource_storage" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "allocates_time_delta_resource_storage")

    Invoke-CheckedCommand `
        -Name "stores_time_delta_resource_payload" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "stores_time_delta_resource_payload")

    Invoke-CheckedCommand `
        -Name "retrieves_time_delta_resource_payload" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "retrieves_time_delta_resource_payload")

    Invoke-CheckedCommand `
        -Name "debug_inspects_time_delta_resource" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "debug_inspects_time_delta_resource")

    Invoke-CheckedCommand `
        -Name "creates_archetype_table_for_position" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "creates_archetype_table_for_position")

    Invoke-CheckedCommand `
        -Name "world_gets_or_creates_position_archetype" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "world_gets_or_creates_position_archetype")

    Invoke-CheckedCommand `
        -Name "inserts_entity_into_position_archetype" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "inserts_entity_into_position_archetype")

    Invoke-CheckedCommand `
        -Name "copies_position_payload_into_column" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "copies_position_payload_into_column")

    Invoke-CheckedCommand `
        -Name "debug_inspects_spawned_position_world" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "debug_inspects_spawned_position_world")

    Invoke-CheckedCommand `
        -Name "allocates_position_component_column" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "allocates_position_component_column")

    Invoke-CheckedCommand `
        -Name "world_create_destroy_smoke" `
        -Executable "cargo" `
        -Arguments @("test", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "world_create_destroy_smoke")

    Invoke-CheckedCommand `
        -Name "archec0 --help" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", "--help")

    Invoke-CheckedCommand `
        -Name "archec0 --version" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", "--version")

    Invoke-CheckedCommand `
        -Name "archec0 examples/exit42.arc" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\exit42.arc")

    $tokenOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/exit42.arc --emit-tokens" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\exit42.arc", "--emit-tokens"))

    Assert-LinesEqual `
        -Name "exit42 token stream" `
        -Actual $tokenOutput `
        -Expected @(
            "Keyword(world)",
            "Identifier(Main)",
            "Keyword(startup)",
            "LeftBrace",
            "Keyword(exit)",
            "Integer(42)",
            "RightBrace",
            "Eof"
        )

    $astOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/exit42.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\exit42.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "exit42 AST" `
        -Actual $astOutput `
        -Expected @(
            "Program",
            "  world Main",
            "  startup",
            "    exit",
            "      integer 42"
        )

    $exit007AstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/exit007.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\exit007.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "exit007 AST" `
        -Actual $exit007AstOutput `
        -Expected @(
            "Program",
            "  world Main",
            "  startup",
            "    exit",
            "      integer 7"
        )

    $let40AstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/let40.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\let40.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "let40 AST" `
        -Actual $let40AstOutput `
        -Expected @(
            "Program",
            "  world Main",
            "  startup",
            "    let x: i32",
            "      integer 40",
            "    exit",
            "      integer 0"
        )

    $mathAstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/math.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\math.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "math AST" `
        -Actual $mathAstOutput `
        -Expected @(
            "Program",
            "  world Main",
            "  startup",
            "    let x: i32",
            "      binary +",
            "        integer 40",
            "        integer 2",
            "    exit",
            "      identifier x"
        )

    $sub42AstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/sub42.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\sub42.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "sub42 AST" `
        -Actual $sub42AstOutput `
        -Expected @(
            "Program",
            "  world Main",
            "  startup",
            "    let x: i32",
            "      binary -",
            "        integer 50",
            "        integer 8",
            "    exit",
            "      identifier x"
        )

    $mul42AstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/mul42.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\mul42.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "mul42 AST" `
        -Actual $mul42AstOutput `
        -Expected @(
            "Program",
            "  world Main",
            "  startup",
            "    let x: i32",
            "      binary *",
            "        integer 6",
            "        integer 7",
            "    exit",
            "      identifier x"
        )

    $positionAstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/position.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\position.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "position AST" `
        -Actual $positionAstOutput `
        -Expected @(
            "Program",
            "  world Demo",
            "  component Position",
            "    field x: f32",
            "    field y: f32",
            "  startup",
            "    exit",
            "      integer 0"
        )

    $spawnPositionAstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/spawn_position.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\spawn_position.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "spawn_position AST" `
        -Actual $spawnPositionAstOutput `
        -Expected @(
            "Program",
            "  world Demo",
            "  component Position",
            "    field x: f32",
            "    field y: f32",
            "  startup",
            "    spawn",
            "      component Position",
            "        field x",
            "          float 1.0",
            "        field y",
            "          float 2.0",
            "    exit",
            "      integer 0"
        )

    $timeDeltaAstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/time_delta.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\time_delta.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "time_delta AST" `
        -Actual $timeDeltaAstOutput `
        -Expected @(
            "Program",
            "  world Demo",
            "  resource Time",
            "    field delta: f32",
            "  startup",
            "    resource Time",
            "      field delta",
            "        float 1.0",
            "    exit",
            "      integer 0"
        )

    $moveSystemAstOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/move_system.arc --emit-ast" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\move_system.arc", "--emit-ast"))

    Assert-LinesEqual `
        -Name "move_system AST" `
        -Actual $moveSystemAstOutput `
        -Expected @(
            "Program",
            "  world Demo",
            "  component Position",
            "    field x: f32",
            "    field y: f32",
            "  component Velocity",
            "    field x: f32",
            "    field y: f32",
            "  resource Time",
            "    field delta: f32",
            "  system Move",
            "    param time: read Time",
            "    param movers: query",
            "      mut Position",
            "      read Velocity",
            "    body",
            "      for",
            "        bindings",
            "          binding pos",
            "          binding vel",
            "        in movers",
            "        body",
            "          add_assign",
            "            target",
            "              field x",
            "                identifier pos",
            "            value",
            "              binary *",
            "                field x",
            "                  identifier vel",
            "                field delta",
            "                  identifier time",
            "          add_assign",
            "            target",
            "              field y",
            "                identifier pos",
            "            value",
            "              binary *",
            "                field y",
            "                  identifier vel",
            "                field delta",
            "                  identifier time",
            "  schedule Main",
            "    run Move",
            "  startup",
            "    resource Time",
            "      field delta",
            "        float 1.0",
            "    spawn",
            "      component Position",
            "        field x",
            "          float 1.0",
            "        field y",
            "          float 2.0",
            "      component Velocity",
            "        field x",
            "          float 3.0",
            "        field y",
            "          float 4.0",
            "    run Main",
            "    exit",
            "      integer 0"
        )

    $positionInspectOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/position.arc --inspect-components" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\position.arc", "--inspect-components"))

    Assert-LinesEqual `
        -Name "position component inspection" `
        -Actual $positionInspectOutput `
        -Expected @(
            "component Demo.Position",
            "  size: 8",
            "  align: 4",
            "  fields:",
            "    x: f32 @ 0",
            "    y: f32 @ 4"
        )

    Invoke-CheckedCommand `
        -Name "archec0 examples/math.arc --check" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\math.arc", "--check")

    $mathMachineOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/math.arc --emit-machine" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\math.arc", "--emit-machine"))

    Assert-LinesEqual `
        -Name "math machine" `
        -Actual $mathMachineOutput `
        -Expected @(
            "function startup",
            "  local x: i32 slot 0",
            "  %0 = i32.const 40",
            "  %1 = i32.const 2",
            "  %2 = i32.add %0, %1",
            "  store slot 0, %2",
            "  %3 = load slot 0",
            "  exit %3"
        )

    $mathCoreOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/math.arc --emit-core" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\math.arc", "--emit-core"))

    Assert-LinesEqual `
        -Name "math Core" `
        -Actual $mathCoreOutput `
        -Expected @(
            "world Main",
            "",
            "function startup {",
            "  local x: i32",
            "  %0 = i32.const 40",
            "  %1 = i32.const 2",
            "  %2 = i32.add %0, %1",
            "  local.store x, %2",
            "  %3 = local.load x",
            "  exit %3",
            "}"
        )

    $moveSystemCoreOutput = @(Invoke-CheckedCommandWithOutput `
        -Name "archec0 examples/move_system.arc --emit-core" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\move_system.arc", "--emit-core"))

    Assert-LinesEqual `
        -Name "move_system Core" `
        -Actual $moveSystemCoreOutput `
        -Expected @(
            "world Demo",
            "",
            "system Move {",
            "  param time: read Demo.Time id 0x7924ce11db524521",
            "  param movers: query",
            "    mut Demo.Position id 0x002202c6aeb4f27b",
            "    read Demo.Velocity id 0x2cf8a68bcb7f913b",
            "  for movers {",
            "    bind pos: mut Demo.Position id 0x002202c6aeb4f27b",
            "    bind vel: read Demo.Velocity id 0x2cf8a68bcb7f913b",
            "    add_assign pos.x, f32.mul vel.x, time.delta",
            "    add_assign pos.y, f32.mul vel.y, time.delta",
            "  }",
            "}",
            "",
            "function startup {",
            "  spawn",
            "    component Demo.Position id 0x002202c6aeb4f27b",
            "      field x = f32.bits 0x3f800000",
            "      field y = f32.bits 0x40000000",
            "    component Demo.Velocity id 0x2cf8a68bcb7f913b",
            "      field x = f32.bits 0x40400000",
            "      field y = f32.bits 0x40800000",
            "  %0 = i32.const 0",
            "  exit %0",
            "}"
        )

    Remove-Item -LiteralPath ".\build\exit42" -Force -ErrorAction SilentlyContinue

    Invoke-CheckedCommand `
        -Name "archec0 examples/exit42.arc -o build/exit42" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\exit42.arc", "-o", ".\build\exit42")

    Test-Elf64Executable -Path ".\build\exit42" -ExpectedExitCode 42
    Test-LinuxExitCode -Path ".\build\exit42" -ExpectedExitCode 42

    Remove-Item -LiteralPath ".\build\exit7" -Force -ErrorAction SilentlyContinue

    Invoke-CheckedCommand `
        -Name "archec0 examples/exit7.arc -o build/exit7" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\exit7.arc", "-o", ".\build\exit7")

    Test-Elf64Executable -Path ".\build\exit7" -ExpectedExitCode 7
    Test-LinuxExitCode -Path ".\build\exit7" -ExpectedExitCode 7

    Remove-Item -LiteralPath ".\build\math" -Force -ErrorAction SilentlyContinue

    Invoke-CheckedCommand `
        -Name "archec0 examples/math.arc -o build/math" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\math.arc", "-o", ".\build\math")

    $mathExpectedText = New-AddRuntimeText -Left 40 -Right 2

    Test-Elf64Payload -Path ".\build\math" -ExpectedText $mathExpectedText
    Test-LinuxExitCode -Path ".\build\math" -ExpectedExitCode 42

    Remove-Item -LiteralPath ".\build\position" -Force -ErrorAction SilentlyContinue

    Invoke-CheckedCommand `
        -Name "archec0 examples/position.arc -o build/position" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\position.arc", "-o", ".\build\position")

    Test-PositionComponentMetadata -Path ".\build\position"

    Remove-Item -LiteralPath ".\build\move_system" -Force -ErrorAction SilentlyContinue

    Invoke-CheckedCommand `
        -Name "archec0 examples/move_system.arc -o build/move_system" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\examples\move_system.arc", "-o", ".\build\move_system")

    Test-EcsMetadataPayload -Path ".\build\move_system"
    Test-LinuxExitCode -Path ".\build\move_system" -ExpectedExitCode 47
    Test-CorruptEcsMetadataMagic -Path ".\build\move_system"
    Test-CorruptEcsComponentDescriptorRecord -Path ".\build\move_system"
    Test-CorruptEcsResourceDescriptorRecord -Path ".\build\move_system"
    Test-CorruptEcsSystemDescriptorRecord -Path ".\build\move_system"
    Test-CorruptEcsQueryDescriptorRecord -Path ".\build\move_system"
    Test-CorruptEcsScheduleDescriptorRecord -Path ".\build\move_system"
    Test-CorruptEcsStartupResourceId -Path ".\build\move_system"
    Test-CorruptEcsStartupSpawnComponentCount -Path ".\build\move_system"
    Test-CorruptEcsStartupOperationKind -Path ".\build\move_system"
    Test-CorruptEcsResourcePayload -Path ".\build\move_system"
    Test-CorruptEcsSpawnPayload -Path ".\build\move_system"
    Test-CorruptEcsRunSchedule -Path ".\build\move_system"

    Remove-Item -LiteralPath ".\build\bad" -Force -ErrorAction SilentlyContinue

    $badSyntaxOutput = @(Invoke-CommandExpectFailure `
        -Name "archec0 tests/e2e/bad_syntax.arc rejects syntax" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\tests\e2e\bad_syntax.arc", "-o", ".\build\bad"))

    Assert-OutputContains -Name "bad syntax diagnostic path" -Output $badSyntaxOutput -ExpectedText "bad_syntax.arc"
    Assert-OutputContains -Name "bad syntax diagnostic location" -Output $badSyntaxOutput -ExpectedText "5:1"
    Assert-OutputContains -Name "bad syntax diagnostic code" -Output $badSyntaxOutput -ExpectedText "error[PARSE001]"
    Assert-OutputContains -Name "bad syntax diagnostic message" -Output $badSyntaxOutput -ExpectedText "expected expression after"

    $badArithmeticOutput = @(Invoke-CommandExpectFailure `
        -Name "archec0 tests/e2e/bad_i32_arithmetic.arc rejects type check" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\tests\e2e\bad_i32_arithmetic.arc", "--check"))

    Assert-OutputContains -Name "bad arithmetic diagnostic path" -Output $badArithmeticOutput -ExpectedText "bad_i32_arithmetic.arc"
    Assert-OutputContains -Name "bad arithmetic diagnostic location" -Output $badArithmeticOutput -ExpectedText "4:12"
    Assert-OutputContains -Name "bad arithmetic diagnostic code" -Output $badArithmeticOutput -ExpectedText "error[CHECK001]"
    Assert-OutputContains -Name "bad arithmetic diagnostic message" -Output $badArithmeticOutput -ExpectedText "expected i32 binding for arithmetic expression"

    $badUnknownScheduleRunOutput = @(Invoke-CommandExpectFailure `
        -Name "archec0 tests/e2e/bad_unknown_schedule_run.arc rejects unknown schedule run target" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\tests\e2e\bad_unknown_schedule_run.arc", "--check"))

    Assert-OutputContains -Name "bad unknown schedule run diagnostic path" -Output $badUnknownScheduleRunOutput -ExpectedText "bad_unknown_schedule_run.arc"
    Assert-OutputContains -Name "bad unknown schedule run diagnostic location" -Output $badUnknownScheduleRunOutput -ExpectedText "7:9"
    Assert-OutputContains -Name "bad unknown schedule run diagnostic code" -Output $badUnknownScheduleRunOutput -ExpectedText "error[CHECK001]"
    Assert-OutputContains -Name "bad unknown schedule run diagnostic message" -Output $badUnknownScheduleRunOutput -ExpectedText 'unknown system `Missing` in schedule'

    $badUnknownResourceParamOutput = @(Invoke-CommandExpectFailure `
        -Name "archec0 tests/e2e/bad_unknown_resource_param.arc rejects unknown system resource parameter" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\tests\e2e\bad_unknown_resource_param.arc", "--check"))

    Assert-OutputContains -Name "bad unknown resource param diagnostic path" -Output $badUnknownResourceParamOutput -ExpectedText "bad_unknown_resource_param.arc"
    Assert-OutputContains -Name "bad unknown resource param diagnostic location" -Output $badUnknownResourceParamOutput -ExpectedText "3:24"
    Assert-OutputContains -Name "bad unknown resource param diagnostic code" -Output $badUnknownResourceParamOutput -ExpectedText "error[CHECK001]"
    Assert-OutputContains -Name "bad unknown resource param diagnostic message" -Output $badUnknownResourceParamOutput -ExpectedText 'unknown resource `MissingTime` in system parameter'

    $badUnknownQueryComponentOutput = @(Invoke-CommandExpectFailure `
        -Name "archec0 tests/e2e/bad_unknown_query_component.arc rejects unknown query component" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\tests\e2e\bad_unknown_query_component.arc", "--check"))

    Assert-OutputContains -Name "bad unknown query component diagnostic path" -Output $badUnknownQueryComponentOutput -ExpectedText "bad_unknown_query_component.arc"
    Assert-OutputContains -Name "bad unknown query component diagnostic location" -Output $badUnknownQueryComponentOutput -ExpectedText "3:27"
    Assert-OutputContains -Name "bad unknown query component diagnostic code" -Output $badUnknownQueryComponentOutput -ExpectedText "error[CHECK001]"
    Assert-OutputContains -Name "bad unknown query component diagnostic message" -Output $badUnknownQueryComponentOutput -ExpectedText 'unknown component `MissingComponent` in query'

    $badConflictingQueryAccessOutput = @(Invoke-CommandExpectFailure `
        -Name "archec0 tests/e2e/bad_conflicting_query_access.arc rejects conflicting query access" `
        -Executable "cargo" `
        -Arguments @("run", "--manifest-path", ".\bootstrap\archec0\Cargo.toml", "--", ".\tests\e2e\bad_conflicting_query_access.arc", "--check"))

    Assert-OutputContains -Name "bad conflicting query access diagnostic path" -Output $badConflictingQueryAccessOutput -ExpectedText "bad_conflicting_query_access.arc"
    Assert-OutputContains -Name "bad conflicting query access diagnostic location" -Output $badConflictingQueryAccessOutput -ExpectedText "10:14"
    Assert-OutputContains -Name "bad conflicting query access diagnostic code" -Output $badConflictingQueryAccessOutput -ExpectedText "error[CHECK001]"
    Assert-OutputContains -Name "bad conflicting query access diagnostic message" -Output $badConflictingQueryAccessOutput -ExpectedText 'conflicting query access for component `Position`'

    $e2eTests = @(Get-ChildItem -LiteralPath $e2eDir -Filter "*.ps1" -File | Sort-Object FullName)
    Write-Host "$($e2eTests.Count) e2e tests discovered"

    foreach ($test in $e2eTests) {
        Invoke-CheckedCommand `
            -Name "e2e $($test.Name)" `
            -Executable "powershell" `
            -Arguments @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $test.FullName)
    }

    Write-Host "All checks passed"
}
finally {
    Pop-Location
}
