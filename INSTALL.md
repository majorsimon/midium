# Installing Midium

## macOS

> **Midium is not yet notarized with Apple.** Two extra steps are needed on
> macOS: bypassing Gatekeeper on first launch, and applying a self-signature
> so that per-app volume control works. Both are covered below.

### 1. Download and install

1. Download `Midium_<version>_macos.app.zip` from the [latest release](https://github.com/majorsimon/midium/releases/latest)
2. Unzip it (double-click in Finder, or `unzip Midium_*.app.zip`)
3. Move `Midium.app` to your **Applications** folder

### 2. Self-sign for per-app volume (required)

Per-app volume control requires the audio-input entitlement, which requires
the app to be signed. Run this once in Terminal after installing (and again
after each update):

```bash
curl -fsSL https://raw.githubusercontent.com/majorsimon/midium/main/app/src-tauri/Entitlements.plist \
  -o /tmp/Midium-Entitlements.plist

codesign --deep --force \
  --sign - \
  --options runtime \
  --entitlements /tmp/Midium-Entitlements.plist \
  /Applications/Midium.app
```

### 3. First launch

Right-click `Midium.app` in Applications → **Open** → click **Open** in the
dialog. (This one-time step is needed because the app isn't notarized.)

When Midium opens, macOS will prompt for **microphone/audio access** — click
**Allow**. Running apps will then appear in the Targets dropdown.

> After the first launch you can open Midium normally (double-click).

---

## Windows

1. Download `Midium_<version>_x64-setup.exe` (NSIS) or `Midium_<version>_x64_en-US.msi`
2. Run the installer
3. If Windows SmartScreen shows a warning: click **More info** → **Run anyway**

   > Midium is not yet code-signed with an EV certificate. Per-app volume
   > control works out of the box — no extra steps needed.

---

## Linux

### AppImage (any distro)

```bash
chmod +x Midium_*.AppImage
./Midium_*.AppImage
```

### Debian / Ubuntu (.deb)

```bash
sudo dpkg -i midium_*.deb
sudo apt-get install -f   # fix any missing dependencies
```

**PulseAudio / PipeWire:** make sure `pulseaudio` or `pipewire-pulse` is
running (`pactl info` should return without error).

---

## Building from source

```bash
git clone https://github.com/majorsimon/midium
cd midium/app
npm ci
npm run tauri build
```
