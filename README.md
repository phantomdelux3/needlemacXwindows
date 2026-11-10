# NeedleX — a dynamic-island music player for Windows

A black "notch" at the top of the screen that follows whatever is playing:
Spotify, Apple Music, and any browser tab (YouTube Music, Spotify Web,
SoundCloud, YouTube…) in Chrome, Edge, Brave, Firefox and Opera.

- **Auto-detection** through Windows' media sessions (SMTC): title, artist, album, cover art, position, controls
- **Compact pill** with cover, title and a visualizer driven by the real audio level
- **Peek** animation when the song changes
- **Hover to expand**: big cover, album-tinted "black glass", scrubbable progress, prev / play / next
- **Synced lyrics** from LRCLIB: Apple-Music-style scrolling panel (click a line to seek)
  plus a karaoke **live lyric** pill under the island
- **Beat glow** in the album's colour; classic (notch) or floating (pill) shape
- **Full-screen view**: click the album art in the island
  - cover view: serif clock, big cover over a soft glow in the album's colours, frosted card with spinning record, progress and controls
  - click the cover for word-by-word lyrics in two styles (switch with the **Aa** button):
    **Stars** (constellations over the liquid, zoom-streaked cover) and **Lens** (the cover in depth-of-field focus; each word appears made of the cover's stretched pixels, then settles: small previous word above, big word, cream label for the next word below)
  - click again to go back to the cover; **triple-click on the left or right side** (or press Esc) to exit
  - Space = play/pause, ←/→ = previous/next, Enter = switch cover/lyrics
  - preview in a browser: `src/full.html?mode=lyrics&style=stars` (demo song)
- **Liquid glass** island (refracts the desktop behind it; black like a notch at the top, clear glass toward the bottom) and glass card in full screen
- **Hi-res covers** (up to 2000 px) looked up on iTunes / Deezer, since players only hand Windows small thumbnails
- **Auto full screen when idle**: tray → *Full screen when idle* (Off / 1 / 3 / 5 / 10 / 15 min, default 3). Only while music plays, never over games, full-screen video or presentations
- **Hide notch**: eye button in the open notch, or tray → *Hide notch* (untick to bring it back)
- **Move the notch**: point at its left/right edge, grab the grip and drag; it snaps to the centre like a magnet. Tray → *Re-center notch* resets it
- Click-through everywhere except the island; never steals focus
- Tray menu: shape, live lyrics, hide when idle, beat glow, launch at startup

## Run / build
```
npm install
npm run dev      # development
npm run build    # -> src-tauri/target/release/bundle/nsis/NeedleX_*_x64-setup.exe
```
Needs Rust (MSVC) and Node. WebView2 ships with Windows 10/11.

## Layout
- `src-tauri/src/media.rs` – SMTC watcher (session picking, art, timeline extrapolation, controls)
- `src-tauri/src/lyrics.rs` – LRCLIB search, YouTube title cleanup, LRC parser
- `src-tauri/src/audio.rs` – output peak meter for the visualizer and glow
- `src-tauri/src/hover.rs` – hover detection and click-through window styles
- `src/` – island UI (HTML/CSS/JS)
