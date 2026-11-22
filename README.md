# morgan

**English** · [Español](README.es.md)

A desktop shell for [Hyprland](https://hyprland.org), written in Rust and drawn by its own engine (vello on wgpu): bar,
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
- **Dock** with your most used and open apps, one dot per window and their titles on hover. It
  can hide itself, and never shows up over a game.
- **Control center**: network, Bluetooth, do not disturb, caffeine, night light, power,
  AI quotas, volume per app, microphone, brightness, the media player and your
  notifications. [More below](#the-control-center).
- **Notifications**: a full freedesktop notification server, with do-not-disturb and a
  game mode that keeps quiet while you play.
- **Launcher**: apps, symbols and emoji, clipboard history, calculator and your keybinds.
- **System menu**: settings for morgan and for Hyprland itself (appearance, input,
  keybinds, date and time…), written back to your config. It's also where you create your
  own keybinds (to open an app or run a command) and per-app rules (workspace, floating,
  size, opacity).
- **Lock screen and idle** (`morgan-lock`): replaces hyprlock and hypridle. It locks through
  logind, so `loginctl lock-session` works, and it can lock before suspending.
- **Login screen** (`morgan-greet`, optional): the lock screen, for logging in, on greetd.
  [See below](#login-screen).
- **OSD** for volume, brightness and Caps Lock; a **window overview** (`Super+Tab` or the mouse in the top left corner); a
  **wallpaper picker**; a **calendar** that reads your iCal URLs.
- **Themes**: nine built-in palettes (morgan, Catppuccin, Dracula, Everforest, Gruvbox, Kanagawa,
  Nord, Rosé Pine, Tokyo Night…) or colors taken from your wallpaper, applied to morgan and,
  if you choose, to Hyprland, GTK, kitty, btop and more.
- **Password prompts** for administrator actions: morgan is your polkit agent, so it
  replaces hyprpolkitagent.
- **English and Spanish**, following your locale.

## Drawn by its own engine

Since October 2026 morgan doesn't use GTK. Every surface (bar, control center, notifications,
launcher, system menu, OSD, lock screen) is drawn by morgan itself, with
[vello](https://github.com/linebender/vello) on the GPU. That's what lets animations run on
a thread of their own, so a busy moment never makes a panel stutter, and lets every panel
measure what it holds. What changed is in the [changelog](CHANGELOG.md).

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
  morgan does not work with a `hyprland.conf`. It also runs on
  [Umbriel](https://github.com/noctalia-dev/umbriel), experimentally for now.
- glib 2.84 or newer.
- A GPU with a working Vulkan driver (Mesa's `vulkan-radeon` or `vulkan-intel`, or NVIDIA's):
  morgan draws everything on the GPU.
- The **JetBrainsMono Nerd Font** (`ttf-jetbrains-mono-nerd` on Arch): the interface's icons are glyphs from it, and without it they show as empty boxes. The packages depend on it; with `install.sh`, `morgan setup` offers it.

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
| BlueZ | The Bluetooth card (without `bluez` and its `bluetooth.service` running, the switch does nothing; `morgan setup` installs and enables them) |
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
| `wtype` | Typing emoji from the launcher |
| `libcanberra` and a sound theme | System sounds |
| Papirus icons and `papirus-folders` | Folder colors that follow the theme |
| `uwsm` | Launching apps as systemd units (without it, morgan uses a scope of its own) |
| `xdg-desktop-portal-gtk` | Browsers and Electron apps following the theme's light or dark mode |

**Apps that follow the theme**: morgan ships templates for kitty, GTK, Qt, btop, yazi,
starship, Vesktop (Discord) and Obsidian. You choose which ones it applies in Appearance.
Your own themes go in `~/.config/morgan/themes/<name>/palette.toml`, your own templates in
`~/.config/morgan/templates/`, and extra apps in `~/.config/morgan/themes.toml`. Games
launched from Steam turn on game mode by themselves.

For your own Hyprland rules, morgan's layers are `morgan-bar`, `morgan-center`,
`morgan-calendar`, `morgan-tray`, `morgan-notifications`, `morgan-osd`, `morgan-launcher`,
`morgan-calculator`, `morgan-menu`, `morgan-wallpapers`, `morgan-windows` and `morgan-corner`
(an invisible pixel, the hot corner's).

## Before you install: stop what morgan replaces

morgan is a complete shell. **Anything else that draws a bar, shows notifications, or locks
the screen will fight with it**: two bars on top of each other, notifications going to the
other program, two lock screens, or the screen locking twice.

You **don't need to uninstall** anything. You need to **stop it and keep it from starting
with your session**:

| If you use… | What to do |
|---|---|
| Another shell: **Noctalia (v4 or v5), DankMaterialShell, Caelestia, HyprPanel**, or any Quickshell, AGS or eww setup | Remove the line that starts it from your Hyprland config, or disable its systemd unit (`systemctl --user disable --now <unit>`). |
| A bar: **Waybar** | Same. |
| A notification daemon: **swaync, mako, dunst** | Same. Only one program can own notifications. |
| An OSD: **SwayOSD** | Same. |
| A locker or idle daemon: **hyprlock, hypridle, swaylock, swayidle** | Same. morgan locks and handles idle, including inhibitors from browsers, video players and games. |
| A polkit agent: **hyprpolkitagent, polkit-gnome, polkit-kde-agent** | Same. Only one program can ask for the administrator password. |
| Night light: **wlsunset, gammastep** | Same, if you want morgan's night light (it uses hyprsunset). |
| A wallpaper daemon: **hyprpaper, swaybg** | Same, if you want morgan's wallpaper picker (it uses awww). |

Your **display manager or login screen** (SDDM, GDM, greetd, ly…) is fine: it runs before
your session, and morgan's lock screen is a separate thing. If you want, morgan has its own
(see [Login screen](#login-screen)). uwsm is fine too, and optional.

`morgan setup` (below) looks for these programs, running or set to start, and offers to stop them. It disables their unit, comments out the Hyprland config line that starts them (keeping a copy of the file next to it) or removes them from your startup apps. A line that starts several things at once is left alone, and `setup` tells you which one to change by hand. `morgan setup --undo` brings everything back.

## Install

The quickest way is `morgan-bin`, from the AUR. It ships the latest release already built and doesn't need Rust:

```sh
yay -S morgan-bin
```

`morgan-git` builds the latest commit on `main` on your machine. It also needs `cargo`, `clang` and `hyprland-protocols`, and it's the one to use if you want what hasn't made it into a release yet, or if you're on `aarch64`, since `morgan-bin` is only built for `x86_64`:

```sh
yay -S morgan-git
```

The [changelog](CHANGELOG.md) says what each version brings.

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

It adds one line to the end of `~/.config/hypr/hyprland.lua`, enables the user service, offers to stop whatever competes with morgan, to install what's recommended and to write [morgan's window shortcuts](#window-shortcuts), and starts it. It shows the list and asks before touching anything, and `morgan setup --dry-run` shows what it would do without doing it.

What's recommended is whatever is missing from [What morgan is built around](#what-morgan-is-built-around), plus bluez and power-profiles-daemon when they apply. They're installed with pacman, except `papirus-folders`, which is in the AUR and goes through yay or paru. Switching what manages your network, audio or power is never done on its own. If it finds iwd, PulseAudio or TLP, for example, it writes out the steps to move to NetworkManager, PipeWire or power-profiles-daemon.

### Window shortcuts

`morgan setup` also offers the window shortcuts morgan was built with, for Hyprland (`~/.config/hypr/morgan-atajos.lua`) and for Umbriel (`~/.config/umbriel/morgan-atajos.toml`). Both come from one table, so the two compositors get the same keys. They're separate files you can edit, written once and never rewritten, and `morgan setup --dry-run` shows them before they exist. Keys you already use stay yours: Hyprland skips the ones your config already binds, and Umbriel loads your `user.toml` after them. In Hyprland they set the layout to scrolling, which they assume.

| Keys | |
|---|---|
| `Super` + arrows or `H` `J` `K` `L` | Focus left, down, up, right |
| `Super+Shift` + `Left` `Right` | Move the column |
| `Super+Shift` + `Up` `Down` | Move the window to the workspace above or below |
| `Super+Ctrl` + arrows, adding `Shift` | Focus another monitor, move the window to it |
| `Super+1` to `9`, `Super+0` | Workspaces (`0` is the 10th, for games); with `Ctrl` or `Shift`, move the window there |
| `Super+U` `Super+I`, `Super` + wheel | Next and previous workspace |
| `Super+Q` | Close the window |
| `Super+F` `Super+Shift+F` `Super+T` `Super+W` `Super+C` | Maximize the column, fullscreen, floating, tabs, center |
| `Super+minus` `Super+plus` | Narrower and wider column |
| `Super+Return` `B` `E` `Z` `S` `O` | Kitty, Brave, Yazi, Vesktop, Steam, Obsidian: focus the next window of the app, or open it if it has none |
| `Super+Shift+Return` `Super+Shift+B` | A new Kitty or Brave window |
| `Super+Shift+M` | Mute the microphone |
| `Super+G` `R` `Alt+R` `Alt+Shift+G` | Record, save the replay, replay buffer, overlay (with `gpu-screen-recorder-ui`) |

On Umbriel the cycling keys call `umbriel-ciclar`, which needs `jq`. Change the apps by editing the file.

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

## Login screen

`morgan-greet` is morgan's lock screen turned into a login screen: pick a user and a session,
and the password also unlocks your keyring. It runs on
[greetd](https://sr.ht/~kennylevinsen/greetd/), which has to be installed and enabled.

```sh
sudo morgan-greet install            # shows what it would change, touches nothing
sudo morgan-greet install --apply    # does it, backing up greetd's config
sudo morgan-greet restore --apply    # goes back to the previous config
```

It takes effect on the next boot. If the greeter ever fails to start, Ctrl+Alt+F2 gives you a
text login to run the `restore` from.

The screen runs inside its own compositor, separate from the session you log into: Hyprland or Umbriel. `install` uses whichever is installed and asks which one if both are.

## Uninstall

Before removing the package, `morgan setup --undo` removes the line from `hyprland.lua`, disables the service and brings back what `morgan setup` stopped, which starts again the next time you log in. Then remove the package (`pacman -R morgan-bin` or `morgan-git`). If you installed from source, `./install.sh --desinstalar` does both. Your config in `~/.config/morgan` and the programs `setup` installed stay.

## License

GPL-3.0-or-later. The bundled themes and templates are MIT, with their credits in
[`dist/share/morgan/CREDITOS.md`](dist/share/morgan/CREDITOS.md).
