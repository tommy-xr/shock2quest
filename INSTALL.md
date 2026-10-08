# Install Shock2Quest on Meta Quest

Shock2Quest is an early development build. You need a Meta Quest headset with
Developer Mode enabled, a Windows, macOS, or Linux computer, a USB data cable,
and your own **System Shock 2: 25th Anniversary Remaster** installation.
The APK does not include retail game data. We recommend **SideQuest's desktop
Advanced Installer** to install the APK and copy your game files without a
terminal or a separate ADB installation. Command-line instructions are also
included below.

## Download and install

### SideQuest (recommended)

1. Open [Get SideQuest](https://sidequestvr.com/setup-howto) and download the
   **Advanced Installer** for your computer. This is the desktop app with APK
   installation and file management; the **Web Installer** installs SideQuest's
   in-headset app instead.
2. Install and open SideQuest. Follow its setup guide to enable **Developer
   Mode** on your Quest, including any Meta developer-account setup it requests.
3. Connect the headset to your computer with a USB data cable. Put on the
   headset and accept **Allow USB debugging**, then check that SideQuest reports
   the device as connected and authorized. The file-access prompt is separate
   from USB debugging; accepting only file access is not enough.
4. Open the [latest release](https://github.com/tommy-xr/shock2quest/releases/latest)
   and download `shock2quest-<version>.apk` to your computer.
5. In SideQuest's top toolbar, choose **Install APK file from folder on
   computer**, select the downloaded APK, and wait for its task to finish
   successfully. Use the APK installer, not the file manager, for this step.
   The APK does not need to be listed in SideQuest's catalog.
6. Keep the headset connected and [copy your game files](#copy-your-game-files)
   before launching Shock2Quest.

If SideQuest cannot connect, check Developer Mode, the USB debugging prompt,
and that your cable supports data. Follow SideQuest's setup troubleshooting for
any required Windows drivers.

### ADB (alternative)

If you prefer a terminal, download and extract
[Android Platform Tools](https://developer.android.com/tools/releases/platform-tools)
for your computer; Android Studio is not needed. Add the extracted
`platform-tools` directory to your `PATH` so the `adb` commands below work from
your download and game directories. Alternatively, replace `adb` in each
command with the full path to the extracted executable. Enable Developer Mode
using the [SideQuest setup guide](https://sidequestvr.com/setup-howto).

1. Open the [latest release](https://github.com/tommy-xr/shock2quest/releases/latest)
   and download `shock2quest-<version>.apk`. In the commands below, replace
   `<version>` with the version you downloaded (without the tag's `v` prefix).
2. Connect your headset by USB, put it on, and accept the USB debugging prompt.
   Run `adb devices` and confirm the headset is listed as `device`.
3. Install the downloaded APK from your download directory:

   ```sh
   adb install -r "shock2quest-<version>.apk"
   ```

   `-r` updates an existing installation
   while retaining app data, provided it uses the same signing key.

### Optional download verification

For either installation method, optionally download `SHA256SUMS` and `INSTALL.md`
into the same directory as the APK and
verify with `sha256sum --check SHA256SUMS` (Linux) or
`shasum -a 256 --check SHA256SUMS` (macOS). On Windows, compare
`Get-FileHash "shock2quest-<version>.apk" -Algorithm SHA256` with the APK entry.

## Copy your game files

Locate your Remaster installation folder on your computer — the folder
containing `sshock2.kpf`, `mods/`, and `cutscenes/`. In Steam, use **Manage →
Browse local files** on the game.

### With SideQuest

1. Open SideQuest's **file manager** (the folder icon in the top toolbar).
2. Open the headset's shared internal storage, shown as `/sdcard/`, and create
   a folder named `shock2quest` if it does not already exist.
3. Upload `sshock2.kpf` into `/sdcard/shock2quest/`.
4. Create `/sdcard/shock2quest/mods/` and upload all the `.kpf` files from your
   computer's `mods/` folder into it.
5. Copy the entire `cutscenes/` folder into `/sdcard/shock2quest/`, preserving
   its subfolders, including `enhanced/`. If your SideQuest version only offers
   file uploads, create the matching folders in its file manager and upload
   each folder's files into the corresponding destination.
6. Wait for all transfer tasks to finish successfully. [Check the folder
   layout](#check-transferred-files): `sshock2.kpf` must sit directly inside
   `shock2quest`, and the videos must not end up in an extra
   `cutscenes/cutscenes/` folder.

### With ADB

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
intact.

Copying all root KPF files is convenient but includes unused files such as the
bonus vault. To save storage and transfer time, you can replace the root `*.kpf`
copy with `adb push sshock2.kpf /sdcard/shock2quest/`; keep the mods and cutscenes
commands.

### Check transferred files

After either copy method, the headset should contain:

```text
/sdcard/shock2quest/
  sshock2.kpf
  ...other root KPF files...
  mods/
    ...mod KPF files...
  cutscenes/
    ...original subfolders and videos...
```

Other root KPF files are optional. These game files only need copying once
unless your game installation changes; APK updates do not require copying them
again.

## Launch and update

Open **Shock2Quest** from your headset's app library (look under **Unknown
Sources** for sideloaded apps). Accept file-access permission if requested.
See the [README controls](https://github.com/tommy-xr/shock2quest#controls).

To update with SideQuest, download the newer APK and use **Install APK file from
folder on computer** again. Install over the existing app to retain app data
when the signing key matches; there is no need to uninstall or recopy your game
files. With ADB, run `adb install -r` with the new filename.
If Android reports `INSTALL_FAILED_UPDATE_INCOMPATIBLE`, your installed build
was signed with a different key. Back up saves before uninstalling it; an
uninstall removes app-private data. Then install the public release. If Android
reports `INSTALL_FAILED_VERSION_DOWNGRADE`, a development build may have a higher
version code; back up before replacing it rather than forcing a downgrade.
