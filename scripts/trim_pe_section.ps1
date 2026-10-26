# Writes a copy of a PE with one section's raw data dropped (turned into uninitialized memory).
# Virtual addresses and SizeOfImage are unchanged, so addresses in the copy match the original dump.
param(
    [Parameter(Mandatory)][string]$In,
    [Parameter(Mandatory)][string]$Out,
    [Parameter(Mandatory)][int]$SectionIndex
)
$ErrorActionPreference = 'Stop'
$src = [IO.File]::OpenRead($In)
$hdr = New-Object byte[] 4096
[void]$src.Read($hdr, 0, 4096)
$pe  = [BitConverter]::ToInt32($hdr, 0x3C)
$n   = [BitConverter]::ToUInt16($hdr, $pe + 6)
$tab = $pe + 24 + [BitConverter]::ToUInt16($hdr, $pe + 20)
$hdrSize = [BitConverter]::ToUInt32($hdr, $pe + 24 + 60)

$secs = for ($i = 0; $i -lt $n; $i++) {
    $s = $tab + 40 * $i
    [pscustomobject]@{ Idx = $i; Ent = $s
        RSize = [BitConverter]::ToUInt32($hdr, $s + 16); ROff = [BitConverter]::ToUInt32($hdr, $s + 20) }
}

$dst = [IO.File]::Create($Out)
$head = New-Object byte[] $hdrSize
$src.Position = 0; [void]$src.Read($head, 0, $hdrSize)

$buf = New-Object byte[] 4MB
$cursor = [uint32]$hdrSize
$dst.Position = $hdrSize
foreach ($s in $secs) {
    $newOff = $cursor; $newSize = $s.RSize
    if ($s.Idx -eq $SectionIndex) { $newSize = 0; $newOff = 0 }
    else {
        $src.Position = $s.ROff; $left = [long]$s.RSize
        while ($left -gt 0) {
            $c = [int][math]::Min($left, $buf.Length)
            $r = $src.Read($buf, 0, $c); if ($r -le 0) { break }
            $dst.Write($buf, 0, $r); $left -= $r
        }
        $cursor += $s.RSize
    }
    [BitConverter]::GetBytes([uint32]$newSize).CopyTo($head, $s.Ent + 16)
    [BitConverter]::GetBytes([uint32]$newOff).CopyTo($head, $s.Ent + 20)
}
$dst.Position = 0; $dst.Write($head, 0, $hdrSize)
$dst.Close(); $src.Close()
"wrote $Out ({0:N1} MB)" -f ((Get-Item $Out).Length / 1MB)
