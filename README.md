# morgan

**English** · [Español](README.es.md)

A desktop shell for [Hyprland](https://hyprland.org), written in Rust with GTK 4: bar,
control center, notifications, launcher, OSD, lock screen and idle, in one process that
shares one look.

https://github.com/user-attachments/assets/2c7eb327-ed32-4eef-89ac-40cdce4034c0

## Why morgan

I built morgan for my own desktop, after living with the two usual ways of putting one
together on Hyprland:

- **Complete shells**, like Noctalia and DankMaterialShell. They look great and do a lot,
  but the look and the behavior are theirs: going past the options they expose meant
  fighting them instead of configuring them.
- **A shell made of separate programs**: rofi or walker, swaync or mako, Waybar, SwayOSD,
  hyprlock. Each one can be styled, but each one is a closed box. A toggle stores its own
  state instead of asking the system, so two outputs that exclude each other can both end
  up checked. There's no per-app audio, panels don't grow with their content or respect the
  edge of the screen, the styling stops where each program stops, every menu is a new
  process, and none of them knows what the others are doing.

morgan is one process with one look that asks the system what's going on, and draws
everything the way I wanted it.

## Before anything: it's built for me

morgan covers **my** needs, on **my** machine, and that's the bar it was built to. It is
**not very customizable**: there are no modules to add or rearrange, the layout is fixed,
and the look changes through the theme's colors, not through a stylesheet of yours. Some
choices only make sense for how I use my computer. If you want a shell to shape into your
own, Noctalia, DankMaterialShell or a modular setup will serve you better. If what you see
here is what you want, welcome.

## What's in it

- **Bar** with workspaces, tray, clock (with a calendar and weather popover) and a privacy
  indicator when the microphone or screen capture is in use.
- **Control center**: network, Bluetooth, do not disturb, caffeine, night light, power,
  AI quotas, volume per app, microphone, brightness, the media player and your
  notifications. [More below](#the-control-center).
- **Notifications**: a full freedesktop notification server, with do-not-disturb and a
  game mode that keeps quiet while you play.
- **Launcher**: apps, symbols and emoji, clipboard history, calculator and your keybinds.
- **System menu**: settings for morgan and for Hyprland itself (appearance, input,
  keybinds…), written back to your config.
- **Lock screen and idle** (`morgan-lock`): replaces hyprlock and hypridle. It locks through
  logind, so `loginctl lock-session` works, and it can lock before suspending.
- **OSD** for volume, brightness and Caps Lock; a **window overview** (`Super+Tab`); a
  **wallpaper picker**; a **calendar** that reads your iCal URLs.
- **Themes**: nine built-in palettes (morgan, Catppuccin, Dracula, Everforest, Gruvbox, Kanagawa,
  Nord, Rosé Pine, Tokyo Night…) or colors taken from your wallpaper, applied to morgan and,
  if you choose, to Hyprland, GTK, kitty, btop and more.
- **English and Spanish**, following your locale.

## The control center

The heart of morgan. `Super+N` opens it in the top-right corner. It grows with what it
holds, never past the edge of the screen, and scrolls inside when it has to.

- **Cards**, in two columns, each with its state in words ("Wired", "2 connected"):
  - **Network**: wired and VPN, with the saved connections, and the Wi-Fi radio; with the
    radio on, below it, the saved Wi-Fi networks and the ones in range: an open one
    connects on click and a secured one asks for its password right there; a wrong
    password is reported and isn't saved. A saved one is forgotten by swiping it right, like
    a notification, and swiping it left copies its password (kept out of the clipboard
    history).
  - **Bluetooth**: the adapter and the paired devices, each connected or disconnected on
    its own; swiping one right forgets it. Below them, the devices nearby (searched for
    while the center is open): picking one pairs and connects it, and if it asks to
    confirm or type a code, the box shows up right there.
  - **Do not disturb**, **Caffeine** (the screen stays on) and **Night light**.
  - **Power**: power-saver, balanced or performance.
  - **AI quotas**: what's left of the Claude Code, Codex and Gemini (Antigravity) limits.
  - **Session**: how long the machine has been on, and lock, suspend, log out, reboot and
    power off. The last three ask first, and a double click can't confirm by accident.
- **What's checked is what the system says**, never what was clicked. If something changes
  elsewhere, the card follows.
- **Levels**: the output (click its name to switch outputs), **every app that's playing
  with its own volume**, mute and output, the microphone, and brightness (laptop backlight,
  or external monitors over DDC/CI). Turning the volume with the panel open doesn't pop up
  the OSD: the slider already shows it.
- **The media player**: title (scrolling when it doesn't fit), controls, shuffle and repeat
  when the player has them, and an equalizer drawn behind it while music plays.
- **Notifications**: grouped by app, critical ones first, groups stacked until you open
  them. Tap one to see its actions, swipe right to dismiss it, swipe left to copy its text.
  While the center is open, nothing pops up on top.
- **All of it by keyboard**: Tab and the arrows move, the arrows change a slider, Enter
  opens a list or runs an action, Delete closes a notification, Escape closes the center.
- **Free while closed**: it only listens to audio, network, Bluetooth, power and the
  player while it's open.

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
| `cava` | The equalizer behind the media player in the control center |
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
