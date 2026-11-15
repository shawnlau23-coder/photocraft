#!/usr/bin/env bash
# Build and package PhotoCraft for Linux (<arch> is x86_64, aarch64 or riscv64):
#
#   $DIST/photocraft-<version>-linux-<arch>.AppImage  any distro with glibc >= the build host's
#   $DIST/photocraft-<version>-linux-<arch>.AppImage.zsync  delta updates (needs zsyncmake)
#   $DIST/photocraft-<version>-linux-<arch>.deb       Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/photocraft-<version>-linux-<arch>.rpm       Fedora, openSUSE, RHEL, ...
#   $DIST/photocraft-<version>-linux-<arch>.tar.gz    plain FHS-style tree (bin/, share/)
#
# With SGL_EDITION set in the environment, the AppImage and tarball are the separate "PhotoCraft
# with SGL Tools" product instead: photocraft-sgl-<version>-linux-<arch>.* with its own desktop id,
# icon names, metainfo component and update-info glob. deb/rpm are refused for it (see the note in
# that section). Unset, everything here is upstream's exactly as before.
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"]
#
# Needs: cargo; nfpm for deb/rpm (https://nfpm.goreleaser.com). By default AppImage tooling
# is downloaded at verified pins into $CARGO_TARGET_DIR. Build on an old distro (CI: Ubuntu 22.04,
# glibc 2.35) so the binaries run on newer ones. Optional: desktop-file-validate, appstreamcli,
# zsyncmake (the zsync package) for the AppImage's .zsync.
#
# Cross builds (CI: riscv64, tar.gz only): set CROSS_ARCH (riscv64), CROSS_TARGET (the Rust
# triple), CROSS_COMPILE (binutils prefix, for strip) and optionally EMULATOR (e.g. qemu-riscv64)
# to smoke-test the CLI and run the example that writes its man pages and completions.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
# shellcheck source=verified-download.sh
. "$HERE/verified-download.sh"
# The "PhotoCraft with SGL Tools" edition (SGL_EDITION set by the SGL job, which also sets
# PHOTOCRAFT_SGL_EDITION for cargo so the binary carries the matching app id, settings folder and
# single-instance files - crates/ui-egui/src/os_window.rs, apps/photocraft/src/app_dirs.rs). Every
# installed identity name follows APP_ID: the .desktop file, the metainfo component, the hicolor
# icon names and AppRun's first-run integration all get this edition's own names, so they land
# beside upstream's instead of replacing them, and the artifacts are named photocraft-sgl-* so a
# person can tell the two downloads apart. Unset, everything below is upstream's exactly as before.
if [[ -n "${SGL_EDITION:-}" ]]; then
  APP_ID=ai.storyteller.photocraft.sgl
  APP_NAME="PhotoCraft with SGL Tools"
  BASENAME_PREFIX=photocraft-sgl
else
  APP_ID=ai.storyteller.photocraft
  APP_NAME=PhotoCraft
  BASENAME_PREFIX=photocraft
fi
# The icon artwork is upstream's and is named after upstream's id in the repository; the SGL stage
# renames copies of it (no second set of artwork to keep in step).
ASSET_ID=ai.storyteller.photocraft

SKIP_BUILD=0
FORMATS="appimage deb rpm tar"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="${CROSS_ARCH:-$(uname -m)}"
case "$ARCH" in
  riscv64) DEB_ARCH=riscv64 ;;
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
export PHOTOCRAFT_MAINTAINER="${PHOTOCRAFT_MAINTAINER:-PhotoCraft maintainers <photocraft@storyteller.ai>}"
BASENAME="$BASENAME_PREFIX-$VERSION-linux-$ARCH"

echo "==> $APP_NAME $VERSION for Linux $ARCH ($FORMATS)"

# The man-and-completions example writes photocraft-cli's man pages and shell completions below. It
# is built in its own cargo run: built with the binaries, its dev-dependencies' features would be
# unified into them.
if [[ "$SKIP_BUILD" = 0 ]]; then
  if [[ -n "${CROSS_TARGET:-}" ]]; then
    (cd "$ROOT" && cargo build --release --locked -p photocraft -p photocraft-cli --features heif --bins --target "$CROSS_TARGET")
    (cd "$ROOT" && cargo build --release --locked -p photocraft-cli --features heif --example man-and-completions --target "$CROSS_TARGET")
  else
    (cd "$ROOT" && cargo build --release --locked -p photocraft -p photocraft-cli --features heif --bins)
    (cd "$ROOT" && cargo build --release --locked -p photocraft-cli --features heif --example man-and-completions)
  fi
fi
BIN="$CARGO_TARGET_DIR/${CROSS_TARGET:+$CROSS_TARGET/}release"
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage an FHS tree (shared by every format) -------------------------------------------------
install -Dm755 "$BIN/photocraft" "$STAGE/usr/bin/photocraft"
install -Dm755 "$BIN/photocraft-cli" "$STAGE/usr/bin/photocraft-cli"
"${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/photocraft" "$STAGE/usr/bin/photocraft-cli" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
# The shared-mime package stays upstream's alone: the type definitions (application/x-photocraft,
# image/qoi, ...) are not per-product, and a second copy of the same definitions under another file
# name would leave two packages maintaining one set of types. The SGL .desktop still lists the same
# MimeType= values, so this edition appears in "Open With" without owning anything.
if [[ -z "${SGL_EDITION:-}" ]]; then
  install -Dm644 "$HERE/$APP_ID.mime.xml" "$STAGE/usr/share/mime/packages/$APP_ID.xml"
fi
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$PHOTOCRAFT_BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
if [[ -n "${SGL_EDITION:-}" ]]; then
  # Same artwork, staged under this edition's icon name: the .desktop Icon= key and Wayland's
  # app_id-to-icon lookup both need an icon named after the entry, and renaming copies keeps one
  # set of artwork in the repository instead of two to keep in step.
  for icon in "$STAGE"/usr/share/icons/hicolor/*/apps/"$ASSET_ID".*; do
    mv "$icon" "${icon%"$ASSET_ID".*}$APP_ID.${icon##*.}"
  done
  # The glob above matched nothing if upstream's asset names ever change: fail loudly rather than
  # ship an entry whose Icon= resolves to nothing.
  test -f "$STAGE/usr/share/icons/hicolor/256x256/apps/$APP_ID.png" || {
    echo "error: no $ASSET_ID icon under assets/app-icon/hicolor to rename to $APP_ID" >&2
    exit 1
  }
fi
mkdir -p "$STAGE/usr/share/doc/photocraft"
copy_docs "$STAGE/usr/share/doc/photocraft"
# photocraft-cli's man pages and shell completions, from its clap definition, written by the
# example built above. A cross build runs it through $EMULATOR, like the --version check at the
# end, and leaves them out without one, as a --skip-build run whose prebuilt binaries came without
# the example does. zsh reads site-functions on Fedora and openSUSE; the deb also links the
# completion into Debian's vendor-completions (nfpm.yaml).
CLI_DOCS="$BIN/examples/man-and-completions"
if [[ -n "${CROSS_TARGET:-}" ]] && [[ -z "${EMULATOR:-}" ]]; then
  warn "no EMULATOR to run the $CROSS_TARGET build, so photocraft-cli's man pages and completions are left out"
elif [[ ! -x "$CLI_DOCS" ]]; then
  warn "$CLI_DOCS is not built, so photocraft-cli's man pages and completions are left out"
else
  if [[ -n "${CROSS_TARGET:-}" ]]; then
    "$EMULATOR" "$CLI_DOCS" "$WORK/cli-docs"
  else
    "$CLI_DOCS" "$WORK/cli-docs"
  fi
  install -Dm644 -t "$STAGE/usr/share/man/man1" "$WORK"/cli-docs/*.1
  install -Dm644 "$WORK/cli-docs/photocraft-cli.bash" "$STAGE/usr/share/bash-completion/completions/photocraft-cli"
  install -Dm644 "$WORK/cli-docs/_photocraft-cli" "$STAGE/usr/share/zsh/site-functions/_photocraft-cli"
  install -Dm644 "$WORK/cli-docs/photocraft-cli.fish" "$STAGE/usr/share/fish/vendor_completions.d/photocraft-cli.fish"
fi

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz ------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb / .rpm --------------------------------------------------------------------------------
if has deb || has rpm; then
  if [[ -n "${SGL_EDITION:-}" ]]; then
    # Refuse rather than build one: nfpm.yaml's package name is `photocraft` and its contents are
    # /usr/bin/photocraft and friends, so an SGL .deb/.rpm would file-conflict with - and could
    # replace - an installed upstream PhotoCraft, which is the one thing this edition exists to
    # avoid. A distro package for the fork needs its own package name, install paths and metadata,
    # which is separate packaging work; the AppImage and tarball are self-contained trees.
    echo "error: the SGL edition does not build deb/rpm (upstream's package name and /usr paths would collide); use --formats \"appimage tar\"" >&2
    exit 1
  fi
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export VERSION
  export NFPM_ARCH="$DEB_ARCH"
  # nfpm expands env vars in fields like `version` and `arch`, but not in `contents[].src`.
  # Without the completions (a cross build with no EMULATOR) the deb's zsh symlink would dangle,
  # so its entry is dropped too.
  DROP_ZSH_LINK=
  [[ -f "$STAGE/usr/share/zsh/site-functions/_photocraft-cli" ]] || DROP_ZSH_LINK='\|^  - src: /usr/share/zsh/site-functions/|,/packager: deb/d;'
  sed "${DROP_ZSH_LINK}s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage -----------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/PhotoCraft.AppDir"
  cp -R "$STAGE" "$APPDIR"
  # Preserve the sctk-adwaita MIT notice in the packaged AppImage.
  install -Dm644 "$ROOT/NOTICE" "$APPDIR/usr/share/doc/photocraft/NOTICE"
  # AppRun is a script, not a symlink: it installs the .desktop entry and icons into
  # $XDG_DATA_HOME before launching, so Wayland compositors can resolve the window's
  # app_id to our icon instead of the generic Wayland logo (#593; opt-out env in the
  # script header). Logic is covered by packaging/linux/apprun-test.sh.
  if [[ -n "${SGL_EDITION:-}" ]]; then
    # AppRun writes the entry and icons it finds in the AppDir into $XDG_DATA_HOME on first run,
    # keyed by its DESK_ID: for this edition that must be this edition's id, so the integration
    # lands beside an installed upstream PhotoCraft's entry instead of overwriting it. One sed over
    # the same script keeps that logic (and packaging/linux/apprun-test.sh) single-sourced.
    sed "s|^DESK_ID=.*|DESK_ID=$APP_ID|" "$HERE/AppRun" >"$APPDIR/AppRun"
    chmod 755 "$APPDIR/AppRun"
  else
    install -Dm755 "$HERE/AppRun" "$APPDIR/AppRun"
  fi
  cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
  cp "$ROOT/assets/app-icon/hicolor/256x256/apps/$ASSET_ID.png" "$APPDIR/$APP_ID.png"
  ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

  # Digests from the official GitHub release asset metadata:
  # https://github.com/AppImage/appimagetool/releases/tag/1.9.1
  # https://github.com/AppImage/type2-runtime/releases/tag/20251108
  # Keep both pins together:
  # appimagetool otherwise downloads the mutable latest type2 runtime itself.
  TOOL_VERSION=1.9.1
  RUNTIME_VERSION=20251108
  case "$ARCH" in
    x86_64)
      TOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
      RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
      ;;
    aarch64)
      TOOL_SHA256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
      RUNTIME_SHA256=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444
      ;;
  esac
  mkdir -p "$CARGO_TARGET_DIR"
  TOOL="${APPIMAGETOOL:-$CARGO_TARGET_DIR/appimagetool-$TOOL_VERSION-$ARCH.AppImage}"
  if [[ -n "${APPIMAGETOOL:-}" ]]; then
    verify_appimagetool_override "$TOOL" "$TOOL_SHA256" "$TOOL_VERSION" "$ARCH"
  else
    download_verified \
      "https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-$ARCH.AppImage" \
      "$TOOL" "$TOOL_SHA256" true
  fi
  RUNTIME="$CARGO_TARGET_DIR/type2-runtime-$RUNTIME_VERSION-$ARCH"
  download_verified \
    "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-$ARCH" \
    "$RUNTIME" "$RUNTIME_SHA256"
  # Absolute, because appimagetool runs in $DIST below (CARGO_TARGET_DIR or APPIMAGETOOL may be
  # relative, e.g. target/agent-<name>).
  OUT="$(cd "$DIST" && pwd)/$BASENAME.AppImage"
  APPDIR="$(cd "$APPDIR" && pwd)"
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  RUNTIME="$(cd "$(dirname "$RUNTIME")" && pwd)/$(basename "$RUNTIME")"
  # A .zsync left by an earlier run would hide a missing zsyncmake and describe another file.
  rm -f "$OUT.zsync"
  # Update information (#349): AppImageUpdate, AppImageLauncher and the like read it from the
  # file and fetch only the blocks that changed in a newer release, through the .zsync published
  # next to each AppImage on GitHub Releases. `latest` is the newest published release that is
  # not a pre-release. A fork's builds point at its own releases through GITHUB_REPOSITORY.
  REPO="${GITHUB_REPOSITORY:-storytold/photocraft}"
  # The glob has to match the published file name, which is $BASENAME below: for the SGL edition
  # that is photocraft-sgl-*, so its AppImages look for their own updates, not upstream's.
  UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|$BASENAME_PREFIX-*-linux-$ARCH.AppImage.zsync"
  # Extract-and-run: works without FUSE (containers, CI). The output embeds the static runtime,
  # so users don't need libfuse2 either. With zsyncmake on the host (CI installs the zsync
  # package) appimagetool also writes the .zsync, into its working directory, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream \
    --runtime-file "$RUNTIME" -u "$UPDATE_INFO" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [[ -s "$OUT.zsync" ]]; then
    echo "wrote $OUT.zsync"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

if [[ -z "${CROSS_TARGET:-}" ]]; then
  "$STAGE/usr/bin/photocraft-cli" --version
elif [[ -n "${EMULATOR:-}" ]]; then
  # Set, so malloc_tuning::apply doesn't re-execute the CLI (#8743): under qemu-user that exec goes
  # to the host kernel, which can't run a $CROSS_ARCH binary, and glibc then hands it to /bin/sh.
  MALLOC_TRIM_THRESHOLD_=131072 "$EMULATOR" "$STAGE/usr/bin/photocraft-cli" --version
fi
echo "==> done"
ls -lh "$DIST"
