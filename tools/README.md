# tools/

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
