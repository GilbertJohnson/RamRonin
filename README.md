# RamRonin 🥷

An unhinged stickman ninja who lives on your Windows desktop, runs across your
app windows, naps on title bars, and **visually** bonks memory hogs with whips,
mallets, rubber chickens, chakra orbs and wind shurikens. He also keeps chosen
apps (like Teams) from going idle.

- **One portable exe, about 1.6 MB.** No runtime, no installer needed, no admin
  rights, no network.
- **Low overhead.** About 15 MB of RAM and well under 1% CPU at 30 fps.
- **No background box.** The overlay uses per-pixel alpha, so every pixel
  that isn't the ninja is click-through.

## Using it

RamRonin lives in the **notification area (tray) only**: one icon, no taskbar
button (the Control Center has none either). Exit it from the right-click menu
or the tray icon's menu (*Exit RamRonin*).

| Action | How |
|---|---|
| Quick menu | Right-click the ninja or the tray icon: *Control Center*, *Character ▸*, *Do something ▸*, *Modes ▸*, *Exit* |
| Control Center | Menu ▸ *Control Center…*, **Ctrl+Alt+R**, click the tray icon, or run `RamRonin.exe` again |
| Hide / show | **Ctrl+Alt+H** |
| Pick him up / fling him | Drag him with the left mouse button |
| Memory callout | Double-click him |
| Scripting | `RamRonin.exe --cmd <bonk, callout, orb, shuriken, kunai, clones, fire, ride, home, pause, hide, shinobi, keep, control, exit>` |
| Whop a specific app | `RamRonin.exe --cmd whop chrome.exe` |

### What he does
- **Moves around:** walks, sneaks, moonwalks, runs, does the arms-back ninja
  sprint, rides a horse or car along the taskbar, bounces on a pogo stick,
  flies a jetpack, flips, cartwheels, does handstands, headspins and split
  jumps, and dances.
- **Uses windows:** leaps onto the visible top edges of app windows and sits
  there with his legs dangling. When a window is moved or resized under him he
  rides along, gets pushed by the edge or stumbles. If the window is closed,
  minimized, maximized or covered by another window, he falls off and lands on
  something below.
- **Climbs:** scales the side walls of windows up to their top edge (and
  climbs back down to the taskbar), and stands on **ledges inside apps**: the
  top edges of toolbars, ribbons, panes and a browser's page area (found from
  the app's own child windows; this also works inside maximized windows).
- **Naps:** sleeps with *Zzz* when RAM use is calm.
- **Rests when you're away:** after a few minutes without mouse / keyboard
  input (Behavior tab, default 3 min) he walks to the taskbar and sleeps until
  you're back. Keep-Active's own F15 taps don't count as you being back.
- **Random by design:** every pick is weighted, what he just did is less
  likely next, and jutsu / stunts / rides have random cooldowns, so rare moves
  (jetpack tours, taskbar sprints, red-eye stares, knocking on a window) show
  up every now and then.
- **Memory clouds:** comic speech or thought clouds with real numbers, for
  example `RAM 83% (13.2 GB / 15.8 GB) · Top: Chrome 2.4 GB`.
- **Whops:** when an app goes over your threshold, or system RAM passes your
  alarm level, he goes to wherever that app is shown and whops it with a whip,
  mallet, wrench, crowbar, boxing gloves or rubber chicken, or a jutsu:
  - its window, if any part of it is visible (top edge, or a ledge inside it);
  - otherwise its **taskbar button or tray icon** (minimized, hidden or
    tray-only apps), which flashes red with every hit.

  The whop is visual only: RamRonin never closes, kills or slows down any app.

### Shinobi Edition (toggle in the Control Center or the right-click menu)
The jutsu are original drawings inspired by ninja anime. This is a fan-made
toy with no affiliation.

- Spiral chakra orb (Rasengan-style), charged in the hand, then a dash and
  "RASEN-BONK!"
- Wind-spiral shuriken (Rasen-shuriken-style), held overhead, thrown, and
  explodes. Against a memory hog he sometimes throws it **from across the
  screen** straight at the hog's window or taskbar button.
- Three-tomoe red eye (Sharingan-style): his eye turns red during fights, a
  big eye flash appears, and a red eye streak follows him when he sprints
- Kunai volley (the kunai stick into the window edge), fire breath, and a
  body-flicker teleport in a puff of smoke
- Shadow clones: 2-4 clones appear for 10 seconds and **each does its own
  thing** (dancing, whopping, flips, handstand, pacing, charging an orb,
  throwing kunai, meditating, headspins...), while he stays to show off.
- The red eye no longer fires with every attack: stares come at random, with
  a pause before whatever follows.
- **Substitution:** if you try to grab him he sometimes leaves a log in your
  hand and reappears somewhere else.

### Keep-Active
Pick one or more apps (running-apps list, "+ Teams" button, or type an exe
name). While one of them is running:
1. Windows is asked not to sleep or turn off the display
   (`SetThreadExecutionState`).
2. If you've been idle for the configured time, RamRonin sends a
   **heartbeat** every N seconds. Choose any mix of:
   - **F15 key tap.** No keyboard has an F15 key and no app uses it, but it
     resets the Windows idle timer.
   - **Mouse nudge.** Moves the mouse 1 pixel and straight back; the cursor
     stays where it was. Some apps only count mouse movement.
   - **Bring the app's window forward** (at most every 3 minutes). The new
     Teams may only count activity in its own window. RamRonin briefly
     brings it to the front, then gives focus back to what you had open and
     re-minimizes it if it was minimized.

   It **never** fires while you are typing or using the mouse.
3. Optionally, it relaunches a kept app if it closes.

**Self-test:** *Control Center ▸ Keep-Active ▸ Run self-test…* takes about
46 minutes while you're away from the PC. It tries each method in turn
(F15 only, mouse only, window forward, then nothing as a baseline). It
records the Windows idle timer and your Teams presence, read from Teams' own
log files, in `%APPDATA%\RamRonin\keep-test.log`, which opens when it's done.
Close Teams on your phone first, because the phone app keeps you Available
by itself.

The ninja keeps roaming as usual and now and then pops over to poke the kept
app ("Stay awake, Teams!").

## Characters and packs (extensible)
Built-in characters: **Shinobi (red band)** (the default), Crimson Eye,
Stickman Fighter, Shadow Ninja, Robo-Bonk 3000, Grandpa Rampage and Neon Disco.

A character is pure data, so new ones can be added without rebuilding. Put a
`.json` file in `%APPDATA%\RamRonin\packs`, or use *Control Center ▸ Import
pack…*. The example packs (Sand Ninja, Lime Robot, Ghost Monk) are built into
the exe and copied into that folder on first run:

```json
{
  "id": "sand_ninja", "name": "Sand Ninja", "body": "#5a3b22",
  "head_fill": "#e8b98a", "head": "round|square|masked",
  "eye": "dot|tomoe|visor|shades|slit", "eye_color": "#ffffff",
  "headband": "#3d5a80", "tails": true, "scarf": "#c2873f",
  "afro": null, "beard": null, "cap": null, "antenna": false,
  "aura": "#f2c14e", "speed": 1.1, "size": 1.0, "line_width": 4.6,
  "ninja_run": true, "favorite": "kunai",
  "phrases": ["Sand coffin for your RAM."]
}
```

- `favorite` can be `whip`, `mallet`, `wrench`, `crowbar`, `gloves`,
  `chicken`, `orb`, `shuriken` or `kunai`.
- One file can also hold several characters: `{"characters": [ ... ]}`.
- See [packs-examples/](packs-examples/).

## Install, update, uninstall
- **Portable:** just run `RamRonin.exe` from anywhere.
- **Install:** *Control Center ▸ About & Updates ▸ Install on this PC*. This
  copies the exe to `%LOCALAPPDATA%\Programs\RamRonin`, adds a Start Menu
  shortcut and an *Apps & features* uninstall entry, and can start with
  Windows. Everything is per-user (HKCU), so there are no UAC prompts.
- **Update from file:** *Update from file…* takes a newer `RamRonin.exe`. It
  checks the file's version info (it must be a RamRonin build), asks before a
  downgrade, swaps the exe in place and restarts. Your settings and packs are
  kept. The flow is in `system::update_from_file`; a GitHub releases check can
  later download the asset and call that same function.
- **Uninstall:** *Apps & features ▸ RamRonin*, or `RamRonin.exe --uninstall`.
- **No admin rights, ever:** the manifest says `asInvoker` and everything
  goes to per-user locations, so Windows never shows a UAC prompt.
- **Started from a Store / packaged app?** (for example a terminal inside such
  an app) Windows would silently redirect RamRonin's settings and registry
  into that app's private storage, so Explorer couldn't open the settings
  folder. RamRonin detects this and restarts itself through the Windows shell
  as a normal desktop app.
- **Settings:** `%APPDATA%\RamRonin\config.json`. If something goes wrong,
  details go to `crash.log` in the same folder. Set the environment variable
  `RAMRONIN_DEBUG=1` (or put an empty `ramronin.debug` file next to the exe)
  to also write a `debug.log` of his state.

## Troubleshooting: "I ran it and nothing happened"
- **When it starts, a "RamRonin is running" notification appears.** The tray
  icon may be hidden under the **^** arrow on the taskbar, because Windows 11
  puts new tray icons there. You can pin it from *Settings ▸ Personalization ▸
  Taskbar ▸ Other system tray icons*.
- **Running `RamRonin.exe` again** while it's already running brings up the
  Control Center in front of your other windows. If the running copy is stuck,
  RamRonin says so and offers to end it and start fresh. It no longer exits
  without telling you.
- **If it crashes**, a message box says so, and the details go to
  `%APPDATA%\RamRonin\crash.log`.
- **Every launch writes a few lines to `%APPDATA%\RamRonin\startup.log`**:
  what it decided (started, handed off to a running copy, relaunched itself),
  your monitors, whether the tray icon was added, and where the first frame
  was drawn. Attach that file (and `crash.log`, if there is one) to a bug
  report.
- If Windows Security or Smart App Control blocked the exe, check *Windows
  Security ▸ Protection history*.

## Building

Requirements: Rust (MSVC toolchain) and the Visual Studio Build Tools (for
`rc.exe` and the linker).

```powershell
cd D:\repos\RamRonin
powershell -ExecutionPolicy Bypass -File .\build.ps1   # -> dist\RamRonin.exe + .sha256
```

Or just run `cargo build --release`, which produces
`target\release\RamRonin.exe`. Run `cargo test` for the pack and skin tests.

### Why it doesn't look like a virus to Windows
- The C runtime is statically linked (`.cargo/config.toml`), so the exe runs
  on any Windows 10/11 x64 machine without the VC++ redistributable.
- Full `VERSIONINFO` (company, product, version), an embedded manifest
  (`asInvoker`, PerMonitorV2 DPI, Common Controls v6) and an icon, all
  generated by `build.rs`. The repo contains no binary assets.
- No packers, no UPX, no PowerShell or cmd spawning, no admin rights, no
  network access, and per-user registry keys only.
- **The one thing that removes SmartScreen's "unknown publisher" prompt for
  downloaded files is code signing.** `build.ps1` signs automatically when
  `RAMRONIN_PFX` and `RAMRONIN_PFX_PASS` are set (an OV/EV certificate, or
  Azure Trusted Signing).

## Architecture

```
src/
  main.rs      host window (timer, tray, hotkeys, menu), layered overlay, frame loop
  brain.rs     Ninja: action queue + state machine (Walk, Leap, Sit, Sleep, Whop,
               Jutsu, Stunt, Fall, Dragged…), decision making, layout & drawing
  rig.rs       stick-figure skeleton: Pose (joint angles) -> FK -> Skel; pose library;
               figure painter (heads, eyes, headband tails, scarf, afro, beard…)
  props.rs     weapons, horse, car, pogo, jetpack, chakra orb, wind shuriken,
               tomoe eye, kunai, smoke, log, fireball, comic bursts
  fx.rs        short-lived effects (bursts, poofs, dust, Zzz, speed lines, kunai,
               thrown shuriken, explosions, fire, substitution log)
  gfx.rs       anti-aliased drawing on a premultiplied pixmap (tiny-skia) + text
               (ab_glyph with the system's Segoe UI)
  skins.rs     data-driven characters (built-ins + JSON packs)
  tracker.rs   enumerates app windows in z-order, computes the *visible* parts of
               each window's top edge and inner ledges (occlusion-aware),
               monitors, DPI
  taskbar.rs   background thread: UI Automation on Explorer's taskbars to find
               each app's taskbar button / tray icon
  memory.rs    background thread: GlobalMemoryStatusEx + per-app working sets
               (Toolhelp + GetProcessMemoryInfo), grouped by exe name
  keepalive.rs background thread: Keep-Active watchdog
  control.rs   native tabbed Control Center (bindings table: checkbox/slider <-> config)
  system.rs    single instance, autostart, install/uninstall, update-from-file, packs
  config.rs    settings (serde JSON, every field has a default -> forward compatible)
```

**Each frame** (30 fps by default):
1. The tracker refreshes window state every 160 ms.
2. The Ninja follows the surface he's standing on. For a window, that means
   re-reading its live rect, so window drags and resizes are smooth.
3. The current `Act` is stepped.
4. The pose is solved into a skeleton.
5. The bounding box of the figure, props, effects and cloud is computed.
6. Only that small region is rendered and pushed with `UpdateLayeredWindow`.

The overlay window follows him around, which keeps CPU use tiny. Pixels with
alpha 0 are click-through; a rectangle with alpha 1 around his body makes him
easy to grab without blocking anything visible.

**Adding things:**
- **A new animation:** add a function in `rig::poses` that returns a `Pose`.
- **A new behaviour:** add an `Act` variant, then handle it in `step` and
  `pose`.
- **A new setting:** add a field to `Config` plus a one-line binding in
  `control.rs`.
- **A new character:** a JSON pack, no code needed.
