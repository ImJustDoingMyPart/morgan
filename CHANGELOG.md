# Changelog

**English** · [Español](CHANGELOG.es.md)

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
