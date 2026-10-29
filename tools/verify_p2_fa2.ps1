param(
    [Parameter(Mandatory=$true)][string]$FinalAlertDir,
    [string]$Map='target/p2-smoke/edited.map',
    [string]$Compiler='i686-w64-mingw32-clang++.exe',
    [string]$Output='target/p2-independent/probe',
    [switch]$Generic
)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$probeDir=Join-Path $repo $Output
New-Item -ItemType Directory -Path $probeDir -Force | Out-Null
$sourceZip=Join-Path $FinalAlertDir 'Source Code/TS_RA2_Mission_Editor_SourceCode.zip'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip=[System.IO.Compression.ZipFile]::OpenRead($sourceZip)
try {
    $entry=$zip.GetEntry('3rdParty/xcc/misc/shp_decode.cpp')
    $reader=[System.IO.StreamReader]::new($entry.Open())
    try {$source=$reader.ReadToEnd()}finally{$reader.Dispose()}
} finally {$zip.Dispose()}
$start=$source.IndexOf('int decode80c(')
$end=$source.IndexOf('int decode80(', $start)
if($start -lt 0 -or $end -le $start){throw 'Cannot locate FinalAlert2 decode80c'}
# Include the upstream copyright/license block and the exact portable reader.
$license=$source.Substring(0,$source.IndexOf('*/')+2)
[System.IO.File]::WriteAllText((Join-Path $probeDir 'fa2_decode80.inc'),$license+"`n"+$source.Substring($start,$end-$start))
Copy-Item -LiteralPath (Join-Path $FinalAlertDir 'lzo2.dll') -Destination $probeDir -Force
& $Compiler '-O2' '-fms-extensions' '-static' '-I' $probeDir (Join-Path $PSScriptRoot 'fa2_pack_probe.cpp') '-o' (Join-Path $probeDir 'fa2-pack-probe.exe')
if($LASTEXITCODE -ne 0){throw 'Probe compilation failed'}
$mapPath=(Resolve-Path -LiteralPath (Join-Path $repo $Map)).Path
Push-Location $probeDir
try {
    if($Generic){$report=& './fa2-pack-probe.exe' $mapPath '--generic'}else{$report=& './fa2-pack-probe.exe' $mapPath}
    if($LASTEXITCODE -ne 0){throw 'FinalAlert2 bundled decoders rejected the edited map'}
    [System.IO.File]::WriteAllText((Join-Path $probeDir 'report.json'),($report -join "`n"))
    $report
} finally {Pop-Location}
