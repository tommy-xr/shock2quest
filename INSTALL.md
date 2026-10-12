# Install Shock2Quest on Meta Quest

Shock2Quest is an early development build. You need a Meta Quest headset with
Developer Mode enabled, a computer, and your own **System Shock 2: 25th Anniversary Remaster** installation.
The APK does not include retail game data.

## Desktop installer (Windows / macOS)

1. From the [release assets](https://github.com/tommy-xr/shock2quest/releases/latest),
   download the installer matching your operating system and architecture.
   On Windows, run the `.exe`. On macOS, open the `.dmg`, drag **Shock2Quest
   Installer** into **Applications**, then open it.
2. Connect your headset by USB and accept USB debugging inside the headset.
   The installer shows the detected model and connection status.
3. Choose your Remaster folder and click **Install on Quest**. The matching APK
   and ADB are included; game files come from your purchased copy.

Desktop installers are currently unsigned, so your operating system may block
or warn when opening them. Use manual installation below if needed, or if no
installer matches your computer. **Copy log** and **Save log…** export the
installer’s recent output for troubleshooting.

## Manual download and install

Install [Android Platform Tools](https://developer.android.com/tools/releases/platform-tools)
(`adb`) first.

1. Download [shock2quest.apk](https://github.com/tommy-xr/shock2quest/releases/latest/download/shock2quest.apk)
   from the latest release.
2. Connect your headset by USB, put it on, and accept the USB debugging prompt.
   Run `adb devices` and confirm the headset is listed as `device`.
3. Install the downloaded APK from your download directory:

   ```sh
   adb install -r "shock2quest.apk"
   ```

   `-r` updates an existing installation
   while retaining app data, provided it uses the same signing key.

To verify a download, compare its entry in `SHA256SUMS` with the output of
`sha256sum "filename"` (Linux), `shasum -a 256 "filename"` (macOS), or
`Get-FileHash "filename" -Algorithm SHA256` (Windows). Replace `filename` with
the APK or installer you downloaded; you do not need to download every asset.

## Copy your game files

Open a terminal in your Remaster installation folder — the folder containing
`sshock2.kpf`, `mods/`, and `cutscenes/` — then run the commands for your computer.
You only need to copy the KPF archives and cutscenes, not the entire installation.

**macOS / Linux (Terminal):**

```sh
adb shell mkdir -p /sdcard/shock2quest/mods
adb push *.kpf /sdcard/shock2quest/
adb push mods/*.kpf /sdcard/shock2quest/mods/
adb push cutscenes /sdcard/shock2quest/
```

**Windows (PowerShell):**

```powershell
adb shell mkdir -p /sdcard/shock2quest/mods
Get-ChildItem -File *.kpf | ForEach-Object { adb push $_.FullName /sdcard/shock2quest/ }
Get-ChildItem -File mods/*.kpf | ForEach-Object { adb push $_.FullName /sdcard/shock2quest/mods/ }
adb push cutscenes /sdcard/shock2quest/
```

Copy the whole `cutscenes` folder so its subfolders (including `enhanced/`) stay
intact. The headset should now contain:

```text
/sdcard/shock2quest/
  sshock2.kpf
  ...other root KPF files...
  mods/
    ...mod KPF files...
  cutscenes/
    ...original subfolders and videos...
```

Copying all root KPF files is convenient but includes unused files such as the
bonus vault. To save storage and transfer time, you can replace the root `*.kpf`
copy with `adb push sshock2.kpf /sdcard/shock2quest/`; keep the mods and cutscenes
commands. These game files only need copying once unless your game installation
changes; APK updates do not require copying them again.

## Launch and update

Open **Shock2Quest** from your headset's app library (look under **Unknown
Sources** for sideloaded apps). Accept file-access permission if requested.
See the [Quest player guide](https://github.com/tommy-xr/shock2quest/blob/main/docs/playing.md) for controls and VR interactions.

To update, download the newer APK and run `adb install -r` with its filename.
If Android reports `INSTALL_FAILED_UPDATE_INCOMPATIBLE`, your installed build
was signed with a different key. Back up saves before uninstalling it; an
uninstall removes app-private data. Then install the public release. If Android
reports `INSTALL_FAILED_VERSION_DOWNGRADE`, a development build may have a higher
version code; back up before replacing it rather than forcing a downgrade.
