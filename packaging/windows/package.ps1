<#
.SYNOPSIS
  Build, sign and package PhotoCraft for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    photocraft-<version>-windows-<arch>.msi            installer (WiX v5, per-user by default, per-machine optional)
    photocraft-<version>-windows-<arch>-portable.zip   photocraft.exe + photocraft-cli.exe + portable.txt
    photocraft-<version>-windows-<arch>-symbols.zip    photocraft.pdb + photocraft_cli.pdb (optional, for profilers)

  -Edition sgl builds and packages the separate product "PhotoCraft with SGL Tools" (its own
  UpgradeCode, install folders, ProgIDs and registry keys), which installs alongside — never on
  top of — an upstream PhotoCraft; the files are named photocraft-sgl-* instead. One compilation
  always feeds both the MSI and the portable ZIP for the chosen edition. The binaries themselves
  carry that identity too: PHOTOCRAFT_SGL_EDITION is set for the build, which compiles in the
  SGL settings folder (%APPDATA%\PhotoCraftSGL, recovery in %LOCALAPPDATA%\PhotoCraftSGL), its own
  single-instance lock and its own AppUserModelID (apps/photocraft/src/app_dirs.rs,
  crates/ui-egui/src/os_window.rs). A portable copy still keeps PhotoCraftData beside the exe.

  The binaries link the C runtime statically (+crt-static), so neither the MSI nor the portable
  zip needs the Visual C++ redistributable. Signing is delegated to sign.ps1 (skipped with a
  warning when no signing secrets are set).

  Needs: Rust (MSVC toolchain + the target), the Windows SDK (rc.exe, signtool.exe),
  and WiX v5: dotnet tool install --global wix --version 5.0.2
             wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch x86 -SkipBuild
  pwsh packaging/windows/package.ps1 -Arch arm64     # cross-compiled; needs the MSVC ARM64 build tools
  pwsh packaging/windows/package.ps1 -Arch x64 -Edition sgl   # "PhotoCraft with SGL Tools" product
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [ValidateSet('upstream', 'sgl')] [string] $Edition = 'upstream',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:PHOTOCRAFT_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI ProductVersion is numeric (major.minor.build); pre-release tags are dropped there.
$MsiVersion = ($Version -split '-')[0]

# The sgl edition packages the same binaries as a separate Windows Installer product with its
# own UpgradeCode, folders, ProgIDs and registry anchor, so it can coexist with an upstream
# PhotoCraft install (see photocraft-sgl.wxs). Only the identity/policy files and file names
# change; build, PE checks, icons, signing, ICE50, installer policy, portable and symbols all
# run identically for both editions.
if ($Edition -eq 'sgl') {
  $NameBase = 'photocraft-sgl'; $Wxs = 'photocraft-sgl.wxs'; $Wxl = 'photocraft-sgl.en-us.wxl'; $Policy = 'install-policy-sgl.json'
  # Its portable marker names this edition's own settings and recovery folders, the same split
  # as the .wxs/.wxl/policy pair above. The file inside the ZIP is still called portable.txt:
  # that name is what app_dirs.rs looks for, only its instructions differ per edition.
  $PortableMarker = 'portable-sgl.txt'
  # Compile the SGL identity into the binaries as well as the installer: the app reads this
  # variable at compile time (option_env!) for its settings/recovery folder names and its
  # AppUserModelID, so an installed SGL copy and an installed upstream copy share neither their
  # preferences nor their single-instance hand-off (each forwards documents to its own process).
  # It is deliberately not a runtime variable: that would be user-wide and would outrank the
  # portable marker. -SkipBuild keeps whatever the existing binaries were compiled with.
  $env:PHOTOCRAFT_SGL_EDITION = '1'
} else {
  $NameBase = 'photocraft'; $Wxs = 'photocraft.wxs'; $Wxl = 'photocraft.en-us.wxl'; $Policy = 'install-policy.json'
  $PortableMarker = 'portable.txt'
}

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'x86' { 'i686-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

if (-not $env:PHOTOCRAFT_BUILD_SHA) { $env:PHOTOCRAFT_BUILD_SHA = (git -C $Root rev-parse HEAD 2>$null) }
if (-not $env:PHOTOCRAFT_BUILD_DATE) { $env:PHOTOCRAFT_BUILD_DATE = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd') }

Write-Output "PhotoCraft $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  # Fail the build (rather than warn) if the icon/VERSIONINFO can't be embedded.
  $env:PHOTOCRAFT_REQUIRE_WINRES = '1'
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p photocraft -p photocraft-cli --features heif --target $Target }
}

$Bin = Join-Path $TargetDir "$Target\release"

# Check both binaries' PE headers before packaging. Machine (COFF header) must match -Arch, so an
# x64 build can never ship labelled arm64. Subsystem (optional header): 2 = Windows GUI, 3 = console.
# The app must be GUI (no console window opens with it); the CLI must stay console so its output
# reaches the terminal.
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
foreach ($check in @(@('photocraft.exe', 2), @('photocraft-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}
$Stage = Join-Path $TargetDir "windows-package\$Arch"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $Bin 'photocraft.exe'), (Join-Path $Bin 'photocraft-cli.exe') $Stage

& (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $Stage 'photocraft.exe') (Join-Path $Stage 'photocraft-cli.exe')

# ---- font notices for the MSI ------------------------------------------------------------------
# The portable ZIP already carries these notices; stage the complete files before MSI creation too.
$Docs = Join-Path $Stage 'font-licences'
New-Item -ItemType Directory -Force -Path $Docs | Out-Null
# The existing MSI document group also carries notices for bundled egui font inputs.
Copy-Item -LiteralPath (Join-Path $Root 'NOTICE'), (Join-Path $Root 'ATTRIBUTION.md') -Destination $Docs
$HasCraftFontLicences = 1
if ($env:CRAFT_FONTS_DIR) {
  foreach ($ofl in Get-ChildItem -Path (Join-Path $env:CRAFT_FONTS_DIR 'fonts\*\OFL.txt') -File -ErrorAction SilentlyContinue) {
    Copy-Item -LiteralPath $ofl.FullName -Destination (Join-Path $Docs "OFL-$($ofl.Directory.Name).txt")
    $HasCraftFontLicences = 1
  }
}
# ---- end font notices for the MSI --------------------------------------------------------------

# ---- MSI ---------------------------------------------------------------------------------------
# The setup wizard's art, drawn here from the app icon so the MSI shows PhotoCraft rather than
# WiX's stock bitmaps: a 493x58 banner across the top of the inner pages (the page title is drawn
# over its left side, so that stays white) and a 493x312 backdrop for the Welcome and Finish pages
# (their text sits on the right; the left 164 px are ours). Both must be 24-bit BMPs.
function New-InstallerArt([string] $IconPng, [string] $BannerOut, [string] $DialogOut) {
  Add-Type -AssemblyName System.Drawing
  $paper = [System.Drawing.ColorTranslator]::FromHtml('#efe9dc')
  $accent = [System.Drawing.ColorTranslator]::FromHtml('#2f7bf5')
  $icon = [System.Drawing.Image]::FromFile($IconPng)
  try {
    foreach ($spec in @(@($BannerOut, 493, 58), @($DialogOut, 493, 312))) {
      $bmp = New-Object System.Drawing.Bitmap $spec[1], $spec[2], ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      try {
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.Clear([System.Drawing.Color]::White)
        if ($spec[2] -eq 58) {
          $g.DrawImage($icon, 493 - 46 - 8, 6, 46, 46)
        } else {
          $panel = New-Object System.Drawing.SolidBrush $paper
          $stripe = New-Object System.Drawing.SolidBrush $accent
          $g.FillRectangle($panel, 0, 0, 164, 312)
          $g.FillRectangle($stripe, 160, 0, 4, 312)
          $g.DrawImage($icon, 18, 64, 124, 124)
          $panel.Dispose(); $stripe.Dispose()
        }
      } finally { $g.Dispose() }
      $bmp.Save($spec[0], [System.Drawing.Imaging.ImageFormat]::Bmp)
      $bmp.Dispose()
    }
  } finally { $icon.Dispose() }
}
$Banner = Join-Path $Stage 'installer-banner.bmp'
$Dialog = Join-Path $Stage 'installer-dialog.bmp'
New-InstallerArt (Join-Path $Root 'assets\app-icon\hicolor\256x256\apps\ai.storyteller.photocraft.png') $Banner $Dialog

$Msi = Join-Path $Dist "$NameBase-$Version-windows-$Arch.msi"
& (Join-Path $PSScriptRoot 'check-icons.ps1')
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot $Wxs) -arch $Arch `
    -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
    -culture en-US -loc (Join-Path $PSScriptRoot $Wxl) `
    -d "Version=$MsiVersion" -d "DisplayVersion=$Version" -d "BinDir=$Stage" -d "IconPath=$(Join-Path $Root 'assets\app-icon\photocraft.ico')" `
    -d "BannerBmp=$Banner" -d "DialogBmp=$Dialog" `
    -d "DocsDir=$Docs" -d "HasCraftFontLicences=$HasCraftFontLicences" `
    -o $Msi
}
Invoke-Native 'MSI shortcut icon validation (ICE50)' {
  wix msi validate $Msi -ice ICE50 -intermediateFolder (Join-Path $Stage 'msi-validation')
}
# Inspect the compiled tables and evaluate the duplicate guards with Windows Installer.
# ICE validation alone cannot catch a well-formed condition with its meaning reversed.
& (Join-Path $PSScriptRoot 'check-install-policy.ps1') -MsiPath $Msi -PolicyFile (Join-Path $PSScriptRoot $Policy)
# wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release assets.
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))
& (Join-Path $PSScriptRoot 'sign.ps1') $Msi

# ---- portable zip ------------------------------------------------------------------------------
$Portable = Join-Path $TargetDir "windows-package\$NameBase-$Version-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
foreach ($f in 'README.md', 'LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
# Builds made with craft-fonts (CRAFT_FONTS_DIR, all official releases) embed its OFL-1.1 fonts:
# ship each font's licence as OFL-<family-dir>.txt.
if ($env:CRAFT_FONTS_DIR) {
  Get-ChildItem -Path (Join-Path $env:CRAFT_FONTS_DIR 'fonts') -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $lic = Join-Path $_.FullName 'OFL.txt'
    if (Test-Path $lic) { Copy-Item $lic (Join-Path $Portable "OFL-$($_.Name).txt") }
  }
}
# portable.txt beside photocraft.exe switches on portable mode: settings, presets and recovery
# files go to PhotoCraftData\ next to the exe instead of %APPDATA% (#228; see app_dirs.rs). The
# source file differs by edition because the folders its instructions name do; the name inside
# the archive is the one app_dirs.rs looks for, and stays portable.txt for both editions.
Copy-Item (Join-Path $PSScriptRoot $PortableMarker) (Join-Path $Portable 'portable.txt')
$Zip = Join-Path $Dist "$NameBase-$Version-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
# The archive holds the files themselves. `-Path <folder>` puts the folder *in* the archive, so
# photocraft.exe came out one level down and portable mode needs portable.txt beside the exe
# wherever it is extracted (#7699); `<folder>\*` archives the contents, as windows7.ps1 does in
# VectorCraft. Checked below, since nothing else reads the layout back.
Compress-Archive -Path (Join-Path $Portable '*') -DestinationPath $Zip
$Check = Join-Path $TargetDir 'windows-package\portable-zip-check'
Remove-Item -Recurse -Force $Check -ErrorAction SilentlyContinue
Expand-Archive -Path $Zip -DestinationPath $Check -Force
foreach ($f in 'photocraft.exe', 'photocraft-cli.exe', 'portable.txt') {
  if (-not (Test-Path (Join-Path $Check $f))) { throw "the portable zip has no $f at its root: $Zip" }
}
Remove-Item -Recurse -Force $Check -ErrorAction SilentlyContinue

# ---- debug symbols -----------------------------------------------------------------------------
# The linker writes a PDB beside each exe, and the exe names it without a path (photocraft.pdb,
# photocraft_cli.pdb). They ship as a separate, optional download so profilers (Windows Performance
# Analyzer, Superluminal, Visual Studio) show PhotoCraft's function names for the released
# binaries (#688). Signing doesn't change the PDB signature an exe records, so they match the
# signed exes; a rebuild of the same commit writes a PDB that doesn't.
$Symbols = Join-Path $TargetDir "windows-package\$NameBase-$Version-windows-$Arch-symbols"
Remove-Item -Recurse -Force $Symbols -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Symbols | Out-Null
foreach ($pdb in 'photocraft.pdb', 'photocraft_cli.pdb') {
  $p = Join-Path $Bin $pdb
  if (-not (Test-Path $p)) { throw "$pdb is missing from $($Bin), where the release build writes it" }
  Copy-Item $p $Symbols
}
Copy-Item (Join-Path $PSScriptRoot 'symbols.txt') $Symbols
$SymbolsZip = Join-Path $Dist "$NameBase-$Version-windows-$Arch-symbols.zip"
Remove-Item -Force $SymbolsZip -ErrorAction SilentlyContinue
Compress-Archive -Path $Symbols -DestinationPath $SymbolsZip

# Smoke-test the CLI when this machine can run it. An ARM64 build made on an x64 runner can't run
# here; .github/workflows/windows-arm64.yml installs and runs it on ARM64 instead.
$HostArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($Arch -ne 'arm64' -or $HostArch -eq 'arm64') {
  Invoke-Native 'photocraft-cli --version' { & (Join-Path $Stage 'photocraft-cli.exe') --version }
} else {
  Write-Output "skipping photocraft-cli --version: an $Arch build doesn't run on this $HostArch machine"
}
Get-Item $Msi, $Zip, $SymbolsZip | Format-Table Name, Length
