# Install Shock2Quest on Meta Quest

Shock2Quest is an early development build. You need a Meta Quest headset with
Developer Mode enabled, a computer with [Android Platform Tools](https://developer.android.com/tools/releases/platform-tools)
(`adb`), and your own **System Shock 2: 25th Anniversary Remaster** installation.
The APK does not include retail game data.

## Download and install

1. Open the [latest release](https://github.com/tommy-xr/shock2quest/releases/latest)
   and download `shock2quest-<version>.apk`. For the first release this is
   `shock2quest-0.0.1.apk`.
2. Connect your headset by USB, put it on, and accept the USB debugging prompt.
   Run `adb devices` and confirm the headset is listed as `device`.
3. Install the downloaded APK from your download directory:

   ```sh
   adb install -r shock2quest-0.0.1.apk
   ```

   Substitute the version you downloaded. `-r` updates an existing installation
   while retaining app data, provided it uses the same signing key.

Optionally download `SHA256SUMS` and `INSTALL.md` into the same directory and
verify with `sha256sum --check SHA256SUMS` (Linux) or
`shasum -a 256 --check SHA256SUMS` (macOS). On Windows, compare
`Get-FileHash .\shock2quest-0.0.1.apk -Algorithm SHA256` with the APK entry.

## Copy your game files

From your Remaster installation folder, run:

```sh
adb shell mkdir -p /sdcard/shock2quest/mods
adb push sshock2.kpf /sdcard/shock2quest/
adb push mods/sshock2ee.kpf /sdcard/shock2quest/mods/
adb push mods/400.kpf /sdcard/shock2quest/mods/
adb push mods/shtup.kpf /sdcard/shock2quest/mods/
adb push mods/scp.kpf /sdcard/shock2quest/mods/
adb push mods/patch_ext.kpf /sdcard/shock2quest/mods/
```

Use the files from `mods/` as shown. The root `sshock2ee.kpf` and
`sshock2ee-vault.kpf` are not needed. These game files only need copying once
unless you change your game installation.

## Launch and update

Open **Shock2Quest** from your headset's app library (look under **Unknown
Sources** for sideloaded apps). Accept file-access permission if requested.
See the [README controls](https://github.com/tommy-xr/shock2quest#controls).

To update, download the newer APK and run `adb install -r` with its filename.
If Android reports `INSTALL_FAILED_UPDATE_INCOMPATIBLE`, your installed build
was signed with a different key. Back up saves before uninstalling it; an
uninstall removes app-private data. Then install the public release. If Android
reports `INSTALL_FAILED_VERSION_DOWNGRADE`, a development build may have a higher
version code; back up before replacing it rather than forcing a downgrade.
