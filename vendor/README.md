# VERA20k dependency

`vera20k/` is a Git submodule of <https://github.com/YuriPlanet/vera20k>.
The editor pins the engine through its Git tree entry, rather than a moving branch.
Use `git submodule status` for the revision selected by your editor checkout.

Both `vera20k-core` and `crates/voxel` use this checkout. The latter compiles
the engine's existing CPU voxel renderer sources because those modules currently
belong to the full game client, rather than a separate render library.
A Cargo Git dependency alone would not expose those source paths to the adapter.
When the engine publishes a CPU renderer crate, both can become ordinary Cargo
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
