# Changelog

## 1.1.1

### Fixes
- **RamRonin now starts on PCs that were switched on less than an hour ago.**
  Before, it crashed right after launch and nothing appeared on screen.
- **Keep-Active works with the new Teams.** While you're away, Keep-Active
  now also nudges the mouse by 1 pixel and straight back (the cursor stays
  where it was). Every 3 minutes it also briefly brings the Teams window
  forward, then gives focus back to what you had open. The F15 tap alone did
  not keep the new Teams Available.
- **A double right-click no longer opens the Control Center or exits
  RamRonin.** The menu opens under the pointer, so the second click used to
  pick an item.
- **Running `RamRonin.exe` again brings up the Control Center in front of
  other windows.** Before, it could open hidden behind the active window.

### New
- **GIF characters:** *Control Center ▸ Character ▸ Import GIF…* turns any
  animated GIF into the character. Plain or transparent backgrounds are cut
  out; other GIFs show as a sticker.
- **Keep-Active heartbeat options:** F15 key tap, mouse nudge, and bringing
  the app's window forward, in any mix. Plus a ~46-minute **self-test** that
  logs which method keeps Teams Available on your PC.
- **A "RamRonin is running" notification** on start. Windows 11 hides new tray
  icons under the **^** arrow.
- **Never fails silently:** crashes show a message box, a stuck running copy
  can be ended from a prompt, and every start is logged to
  `%APPDATA%\RamRonin\startup.log`.
- **New character pack fields:** clothes (shirt, vest, lungi, chain, crown,
  curly hair, mustache) and a comedy "panic" run. See
  `packs-examples/vadivelu-stick.json`.
