param([switch]$Check)

$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    # cargo fmt --all also walks local path dependencies outside workspace
    # membership. Select only editor packages so vendor stays untouched.
    $metadataText = & cargo metadata --offline --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $metadata = ($metadataText -join "`n") | ConvertFrom-Json
    $formatArgs = @('fmt')
    foreach ($package in $metadata.packages) {
        if ($metadata.workspace_members -contains $package.id) {
            $formatArgs += @('--package', $package.name)
        }
    }
    if ($Check) { $formatArgs += '--check' }
    & cargo @formatArgs
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
