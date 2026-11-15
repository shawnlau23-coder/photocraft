# macOS DMG window

What Finder shows when the DMG opens: a 660 × 400 pt background with the app icon and the
`Applications` link side by side. `package.sh` copies these files into the image; nothing here is
generated at build time, so the DMG builds with a plain `hdiutil create -srcfolder` (no Finder
scripting on CI, nothing extra installed in the signing job).

| File | What |
|---|---|
| `background.svg` | Source of the background: the app icon (`assets/app-icon/photocraft-small.svg`, linked, not copied) cropped as a cover on the PhotoCraft colour field (`#2f7bf5`), Ink and Paper. Its text (Inter, JetBrains Mono) is outlined, so rendering it needs no fonts. |
| `background.tiff` | The background at 1x (660 × 400 px, 72 dpi) and 2x (1320 × 800 px, 144 dpi) in one HiDPI TIFF (16-colour palette, Deflate, sRGB; about 90 KB). Goes to `.background/background.tiff`. |
| `dmg-layout.DS_Store` | Finder's view settings for the volume: window size, icon size 128, PhotoCraft.app at (326, 205), `Applications` at (574, 205), and the background. Goes to `.DS_Store` in the image; named so it isn't mistaken for (or ignored like) a Finder-generated `.DS_Store`. |
| `dmg-layout.sgl.DS_Store` | The same window for the "PhotoCraft with SGL Tools" volume: that volume's name and that bundle's folder name (`PhotoCraft with SGL Tools.app`), which the background alias and the icon positions carry. Written by `generate.py` in the same run; **not in the repository until someone with the tools below runs it**, and until then `package.sh` builds that DMG without a layout (icons in Finder's default arrangement, no background art) and says so in the build log. |
| `generate.py` | Writes `background.tiff`, `dmg-layout.DS_Store` and `dmg-layout.sgl.DS_Store` from the SVG and the layouts above. |

## The volume name has no version

The mounted volume is called `PhotoCraft` (or `PhotoCraft with SGL Tools`), not
`PhotoCraft <version>`. The layout points at the background through an alias that includes the volume
name, and Finder resolves it by that name: with a versioned name the window keeps its size and icon
positions but shows no background (tested). The same alias is why one layout file cannot serve both
editions. The DMG file name (`photocraft-<version>-macos-<arch>.dmg`, or `photocraft-sgl-…` for the
SGL edition) still carries the version.

## Rules

- **Finder draws the icon labels in black in light and dark mode** when a window has a background,
  so the area under both icons stays light (Paper).
- **Nothing goes inside the icon boxes:** artwork keeps 10 pt clear of each 128 pt icon box and of
  the label strip under it.

## Regenerate

Edit `background.svg` (or the layout constants in `generate.py`), then run, on any OS:

```sh
pip install 'pillow>=12' ds_store==1.3.3 mac_alias==2.2.3
python3 packaging/macos/dmg/generate.py   # needs resvg on PATH
```

All outputs are byte-for-byte reproducible and carry nothing from the machine that ran it: the
SVG renders without any fonts, and each `dmg-layout*.DS_Store` is written from scratch, its background
alias holding only the volume name and `/.background/background.tiff`. If you add text to the SVG,
outline it (`usvg` from resvg converts text to paths). The window is 660 × 432 with Finder's 32 pt
title bar; the content area is 660 × 400.

This follows VectorCraft's `packaging/macos/dmg/` (storytold/vectorcraft#493).
