# Navidog

> An open-source alternative to Navicat, written in Rust

WIP...

[Prerelease download](https://github.com/xhofe/navidog/releases/tag/nightly)

## macOS installation

Free builds are ad-hoc signed before packaging; no Apple developer certificate
or manual re-signing is required. They are not notarized by Apple.

Move `Navidog.app` from the disk image into `~/Applications` (create the folder
if needed) to allow updates without administrator access. `/Applications` also
works when your account can write to the app and its parent directory.

If macOS blocks a trusted download, first try opening it, then use **System
Settings → Privacy & Security → Open Anyway**. macOS may ask again after an
update; ad-hoc signing does not bypass Gatekeeper. See
[Apple's instructions](https://support.apple.com/en-us/102445).

For a local build, run `scripts/bundle-macos.sh`. It creates the signed app and
disk image in `target/bundle/`.

![screenshot](./docs/screenshots/main_grid.webp)
