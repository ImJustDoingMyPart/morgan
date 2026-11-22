# Changelog

**English** · [Español](CHANGELOG.es.md)

## Unreleased

### The tray moves to the dock

Tray icons now live in the dock. The icon of an app that already has a spot there (Steam, Vesktop) joins that icon, and its menu opens with a right click. What is in the dock only because of the tray (OpenRGB, Cachy-Update) goes at the end, after a divider. An app running in the background with no windows gets a hollow dot. The divider between the most used apps and the other open ones is gone.

Right-clicking any dock icon opens a menu with its windows, "New window", the app's actions (such as "New private window" in Brave), "Close all" and its tray options. A middle click closes the window if there is only one. If there are several, it opens the list, and a middle click on a row closes that one. "New window" moved from the middle click to the menu.

On the bar, without the tray arrow, the recording, microphone and screen sharing indicators sit next to the bell. With the dock turned off (`[dock] show = false`), the tray goes back to the bar as before. `[dock] tray_icons` changes the dock icon of a tray item by its `Id` (for example `"Cachy-Update" = "octopi"`).

On Umbriel, if `UMBRIEL_SOCKET` points to a socket that no longer exists (a test Umbriel can leave it that way in the `systemctl --user` environment), morgan uses the session's socket and fixes the variable for the `umbriel` commands it runs. Before, the bar showed every workspace as inactive and the menu could not talk to Umbriel.

## 0.3.2 · 2026-11-22

### morgan-greet on Umbriel, and screens that no longer turn off on their own

morgan-greet now also runs on Umbriel, without depending on Hyprland, and every monitor starts at its highest refresh rate. `morgan-greet install` uses whichever compositor is installed and asks which one if both are. To set it without being asked, `sudo morgan-greet install --compositor umbriel --apply`.

The login screen logs what happens at startup to `/var/lib/morgan-greet/pantalla.log`, which is emptied on every boot. It records, for example, whether the keyboard got in.

If `morgan-lock` exited without unlocking (after a crash, for example), the session stayed marked as locked and the screens turned off after 30 seconds without input. Now morgan sees that the lock is no longer running and clears the mark.

### Umbriel without Hyprland leftovers, and apps in the theme's mode

On Umbriel, the Shortcuts page of the menu, the launcher's Shortcuts mode and recording a new key read Umbriel's own keybinds. They used to ask Hyprland and came up empty. Per-app rules in Desktop are written for Umbriel, except "No animation", which Umbriel doesn't have. The cursor picked in Appearance changes right away, and System info shows the Umbriel version.

Apps follow the theme's mode. morgan sets GNOME's `color-scheme` and `gtk-application-prefer-dark-theme` to dark with dark themes and to light with light ones, Matugen's light variant included. Browsers and Electron apps read it through `xdg-desktop-portal-gtk`, which `morgan setup` now offers. It's turned off with `disabled = ["color-scheme"]` in `themes.toml`.

If Bluetooth is soft-blocked (rfkill), turning it on from the control center unblocks it. If a hardware switch blocks it, the tile says so instead of bouncing back. `morgan-greet install` warns when greetd is missing or another login manager is in charge, with the command to switch.

### Window shortcuts, and fewer broken first runs

`morgan setup` now offers the window shortcuts morgan was built with: focus and column movement, workspaces, monitors, scrolling layout, apps that cycle and screen recording. It writes them for Hyprland (`morgan-atajos.lua`) and for Umbriel (`morgan-atajos.toml`) from one table, as separate files you can edit. Keys you already use are left alone. `morgan setup --undo` moves them aside as `.bak`. On Umbriel, the cycling keys use the new `umbriel-ciclar` script, which needs `jq`.

The icons are glyphs from JetBrainsMono Nerd Font, and without it they showed as empty boxes. `morgan-bin` and `morgan-git` now depend on `ttf-jetbrains-mono-nerd`, `install.sh` warns when it's missing and `morgan setup` offers it. The README also says that the Bluetooth switch needs `bluez` and its service, which `morgan setup` installs and enables.

## 0.3.1 · 2026-11-20

### `morgan setup` gets your system ready

`morgan setup` no longer just warns about what competes with morgan. It looks for 21 programs (bars, notification daemons, idle daemons, lockers, full shells, polkit agents, wallpaper and night light tools), running or set to start, and offers to stop them. It disables their systemd unit, comments out the Hyprland config line that starts them (keeping a copy of the file next to it) or removes them from your startup apps.

It also offers to install whatever is missing from the recommended programs, with pacman, and `papirus-folders` with yay or paru. If you have Bluetooth or no power profiles, those join the list. Switching what manages your network, audio or power is never done on its own: with iwd, PulseAudio or TLP, for example, it writes out the steps to move to NetworkManager, PipeWire or power-profiles-daemon.

Everything is shown in a list before it happens. `morgan setup --dry-run` shows what it would do without doing it, and `morgan setup --undo` brings back what was stopped and removes morgan from startup.

## 0.3.0 · 2026-11-20

### A screen to log in: morgan-greet

morgan now has its own login screen. It's the lock screen, with the same clock, wallpaper and
theme, but for logging in: it runs on [greetd](https://sr.ht/~kennylevinsen/greetd/) and lets
you pick the user and the session one thing at a time, with the password last. Every monitor
starts at its highest refresh rate, and the password also unlocks the GNOME keyring if it's
installed.

It's **optional** and doesn't install itself. `sudo morgan-greet install` shows what it would
change without touching anything; with `--apply` it does it, backing up greetd's config and
its PAM file first. It takes effect on the next boot. To go back: `sudo morgan-greet restore
--apply`, also from a text login (Ctrl+Alt+F2) if the greeter ever fails to start. Each
account's theme and wallpaper are copied to `/var/lib/morgan-greet` whenever they change;
nothing else leaves the account.

### Beyond Hyprland: Umbriel (experimental)

Everything morgan asked of Hyprland now goes through a common layer with one driver per
compositor. The first one besides Hyprland is
[Umbriel](https://github.com/noctalia-dev/umbriel): workspaces, windows, idle and screens,
night light (no hyprsunset needed outside Hyprland), thumbnails, screen sharing, and the
theme's colors, borders, shadows and animations. `morgan setup` writes its config. In the
system menu, Hyprland-only settings show as disabled and say why. With a compositor that has
its own overview, the key and the hot corner open that one.

Hyprland is still the main compositor; on Umbriel morgan works, but may still have gaps.

### New

- **Dock**: at the bottom, with your most used and open apps, a dot per window and a tooltip
  with the titles on hover. It can auto-hide and never shows over a game.
- **Window overview from the corner**: moving the mouse to the top-left corner opens it, with
  an animation that unfolds from the middle. Pick a window with a click. Turn it off in
  General › Preferences.
- **Hear myself**: in the control center's microphone list, listen to your own mic, with your
  voice level on the slider.
- **Lock screen**: with several monitors, the field follows the pointer and the others show
  only the wallpaper. The field no longer has text in it: a failed attempt shows as a shake,
  red, and a notch per attempt.
- **Menus**: the tray menu and the bar and dock tooltips share one look, fit their content and
  get the bar's blur.

### Fixed

- A monitor that reconnects while idle has the screens off is turned off again.
- Picking a window from the overview (or with Mod+Tab) takes you to that window, not just its
  workspace.
- Grey thumbnails without icons in the window overview (a vello bug that affected every
  surface).
- The dock no longer moves the pointer when jumping to a window.

## 0.2.0 · 2026-10-25

### Goodbye, GTK4

morgan no longer uses GTK4 or gtk4-layer-shell. Between October 22 and 24, one surface at a
time, everything moved to an engine of its own: [vello](https://github.com/linebender/vello)
on wgpu for drawing, [parley](https://github.com/linebender/parley) for text and
smithay-client-toolkit for Wayland. The order was the lock screen, the OSD, the bar with the
calendar, the control center with the tray menu, the notifications, the launcher, the system
menu, the calculator, the wallpaper picker, the window overview, and finally startup itself,
which now runs on a plain glib main loop.

Why: GTK draws on the same thread that does the work, so anything slow (reading a
config, talking to D-Bus) froze the animation. Now each group of surfaces has its own thread
and its own Wayland connection, and draws without waiting for the rest of morgan. With the
main thread stuck for 1.5 seconds on purpose, the control center keeps animating without
dropping a frame. The lock screen, measured against its GTK version, locks faster (70 ms
instead of about 100), uses about 40% less memory, and keeps animating while the rest is busy
instead of freezing.

Also replaced along the way: icons are looked up following the freedesktop spec (SVG through
resvg), the wallpaper folder is chosen through the desktop portal, the same file picker every
app uses, and the theme no longer loads a CSS stylesheet.

What it means for you:

- **gtk4 and gtk4-layer-shell are no longer needed.** If nothing else uses them, you can
  remove them.
- **morgan needs a GPU with a working Vulkan driver** (Mesa or NVIDIA).
- Each change redraws only the layer that shows it, so a ticking clock doesn't repaint the
  whole panel.

### New

- **Password prompts (polkit)**: morgan asks for the administrator password itself, in a box
  like the launcher's, and replaces hyprpolkitagent. Cancelling doesn't count as a failed
  attempt.
- **Control center**: connect to new Wi-Fi networks, with the password typed right there (a
  wrong one shakes and turns red); forget a saved network by swiping it right, copy its
  password by swiping it left; pair Bluetooth devices, with the code confirmation in place.
- **System menu**: monitors (resolution, refresh rate, scale, rotation, position, on/off, a
  name of your own, arranged by dragging them); date and time; your own keybinds and per-app
  rules; your own modes (Hyprland submaps); game mode as a section of Notifications.
- **Launcher**: emoji from Unicode and CLDR built in, with Spanish names, in one folder per
  group. rofimoji is no longer needed.
- **Sounds**: themes are read the way the spec says (`Directories`, `Inherits`, aliases), and
  Listen tells you, for each sound, whether the theme has its own or uses the default.
- **Looks**: the soft glow of the calendar on every panel, toast, OSD, lock screen and window;
  workspaces cross-fade with a slight vertical slide.
- **`morgan setup`** also warns about Quickshell, AGS, eww, swayidle, swaylock and Noctalia v5.

### Fixed

- A notification's `replaces_id` only replaces a notification from the same app.
- Idle timers only count keyboard and mouse input, and only input wakes the screens.
- The wallpaper picker gives its memory back when it closes (it used to keep hundreds of MB of
  thumbnails).
- Forgetting a network no longer leaves an empty gap in the list.
- The lock screen fades in over the desktop, and a restarted `morgan-lock` takes over the
  lock it left behind.

### Removed

- Per-app notes.

## 0.1.1 and earlier

See the [releases](https://github.com/ImJustDoingMyPart/morgan/releases).
