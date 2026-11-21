set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]

# Build and package the editor. Defaults to the native target and Cargo version.
release $RA2_RELEASE_TAG="" $RA2_RELEASE_TARGET="":
    cargo run --locked -p ra2-map-release
