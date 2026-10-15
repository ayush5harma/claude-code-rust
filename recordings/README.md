# Recordings

Records `assets/demo.gif` with [VHS](https://github.com/charmbracelet/vhs) in Docker. The README screenshots come from `showcase.sh`, which runs locally against the fake bridge (see below).

## Usage

Needs a running Docker daemon. Run from the repository root:

```bash
docker compose -f recordings/compose.yml build
docker compose -f recordings/compose.yml run --rm demo
```

The build context is the repository root, so the current working tree is recorded. The GIF is written straight to `assets/demo.gif` through the bind mount.

## Screenshots

`showcase.sh` writes `assets/screenshot-{dark,light}-{welcome,mcp}.png`. It builds the debug binary and runs it in VHS against `tests/fixtures/fake-bridge.js` in its `showcase` scenario, so the pictures carry neutral demo data and need neither Docker nor a login. It needs `vhs` (with `ttyd` and `ffmpeg`) and `node` on `PATH`; `nix shell nixpkgs#vhs -c recordings/showcase.sh` works on this repository's macOS setup. The light run sets no `CLAUDE_RS_APPEARANCE`, so it also shows the terminal background query choosing the light palette.

`assets/mascot-{dark,light}.svg` draw the rest pose of the crab (`FERRIS_REST` in `src/ui/welcome.rs`) as one rectangle per run of quadrant sub-pixels; redraw them if the art changes.

## Files

- `demo.tape` — the VHS script: terminal size, theme, and the typed input.
- `compose.yml` — mounts that keep runs reproducible: scripted `CLAUDE.md`, `main` as `.git/HEAD`, pre-accepted trust dialog, and the host `~/.claude/.credentials.json` for auth.
- `Dockerfile` — builds `claude-rs`, the bridge, and the Bun runtime on top of the VHS image.
- `inspection/` — screenshots kept from earlier tuning passes.

## Notes

- The Rust stage pins `rust:1.88-bookworm` and ignores `rust-toolchain.toml`, so the recording builds at MSRV, not at the pinned 1.98.
- `demo.tape` waits on the `Type a message` placeholder and on `Agent SDK workflow` from the scripted answer in `CLAUDE.md`; changing either text breaks the recording.
- Check the result visually after UI changes — `Set Width`/`Set Height` in the tape may need adjusting.
