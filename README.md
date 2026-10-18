# morgan

**English** · [Español](README.es.md)

A desktop shell for [Hyprland](https://hyprland.org), written in Rust with GTK 4: bar,
control center, notifications, launcher, OSD, lock screen and idle, in one process that
shares one look.

<!-- video -->

## What's in it

- **Bar** with workspaces, tray, clock (with a calendar and weather popover) and a privacy
  indicator when the microphone or screen capture is in use.
- **Control center**: network, Bluetooth, do not disturb, caffeine, night light, power,
  volume per app, microphone, brightness, the media player and your notifications.
- **Notifications**: a full freedesktop notification server, with do-not-disturb and a
  game mode that keeps quiet while you play.
- **Launcher**: apps, symbols and emoji, clipboard history, calculator and your keybinds.
- **System menu**: settings for morgan and for Hyprland itself (appearance, input,
  keybinds…), written back to your config.
- **Lock screen and idle** (`morgan-lock`): replaces hyprlock and hypridle. It locks through
  logind, so `loginctl lock-session` works, and it can lock before suspending.
- **OSD** for volume, brightness and Caps Lock; a **window overview** (`Super+Tab`); a
  **wallpaper picker**; per-app **keybind notes**; a **calendar** that reads your iCal URLs.
- **Themes**: nine built-in palettes (morgan, Catppuccin, Dracula, Everforest, Gruvbox, Kanagawa,
  Nord, Rosé Pine, Tokyo Night…) or colors taken from your wallpaper, applied to morgan and,
  if you choose, to Hyprland, GTK, kitty, btop and more.
- **English and Spanish**, following your locale.

## Requirements

- **Arch Linux** or a derivative (other distros: build from source with `install.sh`).
- **Hyprland 0.56 or newer, with its config in Lua** (`~/.config/hypr/hyprland.lua`).
  morgan does not work with a `hyprland.conf`.
- GTK 4.22 or newer and `gtk4-layer-shell`.

## What morgan is built around

morgan doesn't reinvent what other programs already do well: it drives them and gives them
one face. Everything below is optional: morgan runs without it, and the part that needs it
is missing. **These are the programs it was built and tested with.** Using an alternative
usually works for the basics, but expect gaps or bugs there. If you're new to Hyprland, this
list is also a good set of tools to start from.

**System services**, which most Arch setups already have:

| Service | What morgan uses it for |
|---|---|
| PipeWire with `pipewire-pulse` and WirePlumber | Volume, per-app audio, microphone, the privacy indicator |
| NetworkManager | The network card (iwd or systemd-networkd on their own won't show up) |
| BlueZ | The Bluetooth card |
| power-profiles-daemon | The power card |
| systemd-logind | Locking, suspend, power off |

**Tools**:

| Program | What it does in morgan |
|---|---|
| `matugen` | Themes: every theme, built-in or from the wallpaper, is rendered through it |
| `awww` | Wallpapers (hyprpaper and swaybg aren't driven by morgan) |
| `libvips` | Thumbnails in the wallpaper picker |
| `hyprsunset` | Night light |
| `ddcutil` | Brightness of external monitors (DDC/CI) |
| `hyprshot` | Screenshots (`Print`), configured from the system menu |
| GPU Screen Recorder (`gpu-screen-recorder-ui`) | Screen recording: the bar shows the time and stops it with a click. Other recorders don't get that counter |
| `cliphist` (with `wl-clipboard`) | Clipboard history in the launcher |
| `libqalculate` | The calculator |
| `wtype` and `rofimoji` | Typing symbols and emoji from the launcher, and their names |
| `libcanberra` and a sound theme | System sounds |
| Papirus icons and `papirus-folders` | Folder colors that follow the theme |
| `uwsm` | Launching apps as systemd units (without it, morgan uses a scope of its own) |

**Apps that follow the theme**: morgan ships templates for kitty, GTK, Qt, btop, yazi,
starship, Vesktop (Discord) and Obsidian. You choose which ones it applies in Appearance.
Games launched from Steam turn on game mode by themselves.

## Before you install: stop what morgan replaces

morgan is a complete shell. **Anything else that draws a bar, shows notifications, or locks
the screen will fight with it**: two bars on top of each other, notifications going to the
other program, two lock screens, or the screen locking twice.

You **don't need to uninstall** anything. You need to **stop it and keep it from starting
with your session**:

| If you use… | What to do |
|---|---|
| Another shell: **Noctalia, DankMaterialShell, Caelestia, HyprPanel**, or any Quickshell, AGS or eww setup | Remove the line that starts it from your Hyprland config, or disable its systemd unit (`systemctl --user disable --now <unit>`). |
| A bar: **Waybar** | Same. |
| A notification daemon: **swaync, mako, dunst** | Same. Only one program can own notifications. |
| An OSD: **SwayOSD** | Same. |
| A locker or idle daemon: **hyprlock, hypridle, swaylock, swayidle** | Same. morgan locks and handles idle, including inhibitors from browsers, video players and games. |
| Night light: **wlsunset, gammastep** | Same, if you want morgan's night light (it uses hyprsunset). |
| A wallpaper daemon: **hyprpaper, swaybg** | Same, if you want morgan's wallpaper picker (it uses awww). |

Your **display manager or login screen** (SDDM, GDM, greetd, ly…) is fine: it runs before
your session, and morgan's lock screen is a separate thing. uwsm is fine too, and optional.

`morgan setup` (below) warns about the most common of these if they're running, and doesn't
start morgan while another program owns notifications.

## Install

From the AUR, prebuilt or built from the latest commit:

```sh
yay -S morgan-bin     # or: yay -S morgan-git
```

Or from source, into `~/.local` (it checks dependencies and tells you what's missing):

```sh
git clone https://github.com/ImJustDoingMyPart/morgan
cd morgan
./install.sh          # --help for options
```

Then, **once per user, from inside Hyprland**:

```sh
morgan setup
```

It adds one line to the end of `~/.config/hypr/hyprland.lua`, enables the user service,
warns about anything that competes with morgan, and starts it.

## Using it

| Keys | |
|---|---|
| `Super+Space` | Launcher |
| `Super+,` | System menu (all settings) |
| `Super+N` | Control center |
| `Super+Tab` | Window overview |
| `Super+V` | Clipboard history |
| `Super+Alt+U` | Symbols and emoji |
| `Super+Alt+C` | Calculator |
| `Super+Y` | Wallpaper picker |
| `Super+Alt+N` | Keybind notes for the focused app |
| `Super+Shift+N` | Do not disturb |
| `Super+X` | Power off, reboot, log out |
| `Super+Alt+L` | Lock |

All keybinds can be changed from the system menu or in `~/.config/morgan/config.toml`.
`morganctl` controls morgan from scripts and your own binds (`morganctl` with no arguments
lists everything).

## Uninstall

Remove the package (`pacman -R morgan-bin` or `morgan-git`), or run
`./install.sh --desinstalar` if you installed from source. Then delete the line
`pcall(require, "morgan")` from `~/.config/hypr/hyprland.lua`, and re-enable whatever you
stopped before installing. Your config in `~/.config/morgan` is left in place.

## License

GPL-3.0-or-later. The bundled themes and templates are MIT, with their credits in
[`dist/share/morgan/CREDITOS.md`](dist/share/morgan/CREDITOS.md).
