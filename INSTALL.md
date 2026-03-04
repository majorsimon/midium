# Installing Midium

## macOS

### Basic installation

1. Download `Midium_<version>_macos.app.zip` from the [latest release](https://github.com/majorsimon/midium/releases/latest)
2. Unzip it (double-click in Finder, or `unzip Midium_*.app.zip`)
3. Move `Midium.app` to your Applications folder
4. **First launch:** right-click `Midium.app` → **Open** → click **Open** in the dialog

   > Midium is not yet notarized with Apple. Right-clicking bypasses the Gatekeeper
   > warning for unsigned apps. After the first launch you can open it normally.

### Per-app volume control (macOS 14.2 Sonoma or later)

Per-app volume requires the app to have audio capture permission, which in turn
requires the app to be signed with the `com.apple.security.device.audio-input`
entitlement. Until Midium has a Developer ID certificate (notarization), you can
apply an **ad-hoc signature** yourself — this stays local to your machine and is
safe to do.

**One-time setup (paste into Terminal):**

```bash
# 1. Sign the app with the audio-input entitlement
curl -fsSL https://raw.githubusercontent.com/majorsimon/midium/main/app/src-tauri/Entitlements.plist \
  -o /tmp/Midium-Entitlements.plist

codesign --deep --force \
  --sign - \
  --options runtime \
  --entitlements /tmp/Midium-Entitlements.plist \
  /Applications/Midium.app

# 2. Clear any cached permission decision and launch
tccutil reset Microphone com.midium.app
open /Applications/Midium.app
```

When Midium opens, macOS will prompt for microphone/audio access — click **Allow**.
The Running Apps list in the Targets dropdown will then populate.

> **Note:** You need to re-run the `codesign` step after every update since a new
> download replaces the signature.

---

## Linux

### AppImage (any distro)

```bash
chmod +x Midium_*.AppImage
./Midium_*.AppImage
```

Or move it to `~/.local/bin/` and create a desktop shortcut.

**PulseAudio / PipeWire:** Per-app volume uses the default PulseAudio sink. Make
sure `pulseaudio` or `pipewire-pulse` is running (`pactl info` should return
without error).

### Debian / Ubuntu (.deb)

```bash
sudo dpkg -i midium_*.deb
# Fix any missing dependencies:
sudo apt-get install -f
```

Then launch from your application menu or run `midium-app`.

---

## Windows

1. Download `Midium_<version>_x64-setup.exe` (NSIS installer) or `Midium_<version>_x64_en-US.msi`
2. Run the installer
3. If Windows SmartScreen shows a warning, click **More info** → **Run anyway**

   > Midium is not yet code-signed with an EV certificate. SmartScreen warns on
   > unsigned installers; this will be resolved in a future release.

Per-app volume works out of the box on Windows — no extra setup needed.

---

## Building from source

```bash
git clone https://github.com/majorsimon/midium
cd midium/app
npm ci
npm run tauri build
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for full dev setup instructions.
