# VERA20k dependency

`vera20k/` is a Git submodule of <https://github.com/YuriPlanet/vera20k>.
The editor pins the engine through its Git tree entry, rather than a moving branch.
Use `git submodule status` for the revision selected by your editor checkout.

The current integration targets VERA20k 0.3.9 at
`63253c5bb78af4eb7232abab8da727c0d1716bd5` (updated 2026-11-21).
The engine mounts `ECACHE*.MIX` and `ELOCAL*.MIX` through the shared asset
manager. Editor building previews use its RULES slot-9 configuration and
32-facing frame table for healthy SHP turrets; ART `TurretAnim` is ignored.
Static SHP vehicles also draw the engine-selected 32-frame turret ring from
the body SHP, using the authored heading and the body draw point.
Scene rendering retains the building turret's independent Ground sort key and animation
pixel depth, including `TurretAnimYSort`, `TurretAnimZAdjust` and ART draw offsets.
VXL guns retain separate turret/barrel crops and Z seeds. SHP selection adds
ART `Start`, uses the selected animation palette, and retains directional shadow
frames. Packed RGB565 translucency reads the existing framebuffer on both CPU
and GPU. Placement previews use the same offset projection as the scene, and
building art and turrets follow the engine's default (no optional high-terrain
lift). ZShape sampling uses the source frame offset rather than the canvas offset.
Mission audit loaders follow the engine's nonfatal Team AI diagnostics API.
Random-map inputs share the engine's loaded Extras settings for theater fences
and town catalogs, and allow its parallel generation to borrow tile blocks.
Headless mission audits retain simulation resources in the engine's shared `Arc`.
Building voxel guns use the engine's bound physical model slots, including an
explicit `VoxelBarrelFile`, instead of reconstructing filenames in the editor.
Stock team script actions 18, 28, 29, 35, 36, 44 and 45 are available in the
authoring schema and TypeScript SDK. Extended player starts require an explicit
MaxPlayers declaration and all declared waypoints (up to 120). Offline `-SPAWN`
playtests can use all 120 slots with one local human and up to 119 AI opponents.
See the engine map format
[specification](vera20k/docs/map-format.md). Overlay pack dimensions already
follow the map header cell grid, including extended maps with overlays below 512.

The editor uses a single build. Ares mission authoring is always available;
the per-map resource setting controls the launched game. INI syntax, include
expansion and supplemental CSF tables follow the resource manager's `ships_ares()`.
Auto does not mount an installation-only `ares.mix` without a selected mod; On
explicitly selects Ares, and Off disables it. The map document preserves its
dialect across edits, background saves and recovery. Inherited and iterator-key
maps remain read-only until flattened.
Static voxel vehicles use the production Rules owner's Ares `WaterImage`
reference on water off bridges; the actual type still supplies `UseBuffer`.
The scene, picking, export, selection and paste ghosts share this choice.
Water turrets retain the selected type's ART offset independently of its
voxel filename. Overlay land overrides reach the resolved scene, and paste
ghosts select art from the terrain and overlays they will write.
Indexed guns and dynamic unloading, disguise and deployment remain outside
this static selection path. Scheduler animation binding now follows the
engine's nonfatal API in both mission audit loaders.
The renderer adapter declares the engine's private test-group cfg names for
strict linting without enabling client-only tests.

Both `vera20k-core` and `crates/voxel` use this checkout. The latter compiles
the engine's shared CPU/GPU voxel renderer sources because those modules currently
belong to the full game client, rather than a separate render library.
A Cargo Git dependency alone would not expose those source paths to the adapter.
The adapter uses GPU palette-index rasterization on the asset worker and reads
static images back for previews, picking and scene atlases, batching up to 32 poses
per GPU submission. CPU fallback follows the engine admission rules. Multi-part
merging requests the CPU result once because the GPU image has no geometric depth.
No game-client renderer sources are copied or modified.

The extended random-map pipeline also calls the engine's shared
`tech_buildings::run_start_flips_traced` phase before marking start clearings.
This entry point must be public in the selected engine revision; keep its API
change with the engine update rather than copying the placement algorithm here.

When the engine publishes a renderer crate, both can become ordinary Cargo
dependencies without retaining the source adapter.

## Initialize or restore the pinned revision

From the editor repository root:

```powershell
git submodule update --init --recursive
cargo build --locked --workspace
```

For a new checkout, use `git clone --recurse-submodules <editor-repository-url>`.
The engine is an independent workspace, excluded from the editor workspace.
Use `./tools/format_workspace.ps1` (`-Check` to check) for editor formatting.
`cargo fmt --all` also traverses local path dependencies and would format upstream sources.
No sibling `../vera20k` checkout is required.

## Upgrade deliberately

Choose a commit, then validate it before recording the new submodule pointer:

```powershell
git -C vendor/vera20k fetch origin
git -C vendor/vera20k checkout --detach <engine-commit>
cargo test --locked --workspace
cargo clippy --locked --workspace --no-deps --all-targets -- -D warnings
./tools/format_workspace.ps1 -Check
```

Run the map and SHP/VXL window smoke tests with local game assets as documented
in the root README. If manifests changed, regenerate `Cargo.lock` with Cargo
and review its diff before running the locked checks.
Include `vendor/vera20k` and any resulting lockfile changes in the editor's change.
Avoid `git submodule update --remote` in builds and CI: it selects a moving revision.

For engine development, work on a branch inside the submodule or use your
separate engine checkout. Local engine changes are not captured by the editor's
submodule pointer; reproducible editor versions require a committed engine revision
available from the configured remote. This integration does not copy local changes
from another checkout.

VERA20k retains its own license; see [LICENSE-GPL](vera20k/LICENSE-GPL).
