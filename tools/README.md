# tools/

`upx.exe` — [UPX](https://upx.github.io/) 5.2.0 (win64), used by
`npm run pack` to compress the release executable.

It is committed so the packed build works without a separate download. To update
it, replace `upx.exe` with a newer win64 build from
https://github.com/upx/upx/releases.

## Icons

`make-icon.py` regenerates every app icon (desktop, Windows Store tiles,
Android, iOS, and the About box) from `justcode-mark.png`, the source mark.
Run `python tools/make-icon.py`; see the script's docstring for details.

The other files here are inputs/scratch space for that script and aren't
meant to be edited directly:

- `justcode-mark.png` — the "jc" mark, transparent background
- `justcode-mark-android-fg.png` — the mark padded onto a larger transparent
  square so it fits Android's adaptive-icon safe zone
- `android-bg-white.png` — plain white Android adaptive-icon background layer
- `icon-manifest.json` — tells `tauri icon` how to combine the above
  (rewritten by `make-icon.py` on every run)

## Why packing is opt-in

`npm run tauri build` produces a normal, uncompressed `justcode.exe`. Packing is
a separate step (`npm run pack`, or `npm run build:packed` to do both) because:

- A packed exe **self-decompresses at every launch**, so it starts slightly
  slower — the opposite of what you want if boot time matters.
- Packed binaries are a **common Windows Defender / antivirus false-positive**
  trigger.
- The NSIS and MSI installers already compress their payload, so packing the exe
  inside them saves almost nothing; `pack` only shrinks the standalone exe.
