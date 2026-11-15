# Feature checklist (parity target: Viewnior 1.8)

Source of truth: `src/vnr-window.c`, `src/uni-image-view.c`, `src/uni-anim-view.c`,
`src/vnr-prefs.[ch]` and `man/viewnior.1` in <https://github.com/hellosiyan/Viewnior>.

Legend: `[ ]` todo · `[x]` done · `(M#)` target milestone · `+` = beyond Viewnior

## Opening
- [x] Open file(s) / folder(s) from the CLI (`rustnior [FILES|FOLDERS]...`) (M1/M2)
- [x] `--fullscreen`, `--slideshow` CLI flags (M3)
- [x] Open Image… (`Ctrl+O`) and Open Folder… (`Ctrl+F`) file choosers (M2)
- [x] Drag-and-drop files/folders/URIs onto the window (M2)
- [x] "Browse only selected images": opening several files limits the collection to them (M2)
- [ ] Show hidden files option (M6)
- [ ] Reload current file (M5)

## Navigation
- [x] Next/previous in folder: `Space`/`BackSpace`, `Page Down`/`Page Up`, `Alt+Right`/`Alt+Left` (M2)
- [x] `Left`/`Right`/`Up`/`Down` navigate when the image fits the window, otherwise pan (M2)
- [x] First/last: `Home`/`End`, `Alt+Home`/`Alt+End` (M2)
- [x] Wrap around at collection ends (M2)
- [x] Natural sort (+ sort by name/date/size, ascending/descending) (M2, `+`)
- [ ] Watch folder for added/removed files (`+`, M6)

## Zoom & pan
- [x] Best fit (`F`, `0`, `KP_0`), normal size 100% (`1`, `Ctrl+0`), 200% (`2`), 300% (`3`) (M1)
- [x] Zoom in (`+`, `=`, `KP_Add`, `Ctrl++`, `Ctrl+=`), zoom out (`-`, `KP_Subtract`, `Ctrl+-`) (M1)
- [ ] Scroll-wheel behaviour (pref): zoom (default) / navigate / scroll; `Ctrl+wheel` always zooms around the pointer (M1/M6)
- [x] Smooth zoom animation (`+`) (M1)
- [x] Pan by dragging; keyboard panning with arrows (step) and `Shift+arrows` (page) (M1)
- [ ] Initial zoom mode pref: smart (fit if larger, else 100%) / 100% / fit / last used (M1/M6)
- [ ] "Smooth images" pref (filtering on/off) (M6)
- [ ] Adjust window size to the image (M6)

## Transform
- [x] Rotate clockwise `Ctrl+R` (+ `R`), anticlockwise `Ctrl+Shift+R` (+ `L`) (M3)
- [x] Flip horizontal `H`, flip vertical `V` (M3)
- [x] Respect EXIF orientation automatically (M3)
- [ ] On navigating away from a modified image: ask / auto-save / ignore (pref) (M5)

## Modes
- [x] Fullscreen `F11` (toggle), `Esc` leaves; auto-hiding toolbar/cursor (M3)
- [x] Fit image when entering fullscreen (pref) (M3)
- [x] Slideshow `F5`, configurable interval (default 5 s); `Esc` stops (M3)
- [ ] Double-click behaviour (pref): zoom (default, toggles fit↔100%) / fullscreen / next (M3/M6)
- [x] `Esc`/`Q` quits in normal mode, `Ctrl+W` closes window (M1)

## Animation
- [x] Animated GIF, APNG, animated WebP playback (M4)
- [x] `P` pause/resume, `J` step one frame (M4)

## Information
- [x] Properties dialog `Alt+Return`: name, type, dimensions, file size, location, modified date, EXIF/IPTC (M4)
- [x] Status bar: file name, dimensions, zoom %, position in collection, file size (M2)
- [x] Thumbnail strip (`+`, M4)
- [x] Window title shows the file name (M1)

## File operations
- [ ] Save `Ctrl+S` (write rotated/flipped image back), Save as… (`Ctrl+Shift+S`, `+`) (M5)
- [ ] Lossless JPEG rotation (EXIF orientation tag rewrite, or re-encode with a warning) (M5)
- [ ] JPEG quality / PNG compression prefs (M5/M6)
- [ ] Delete `Delete` → trash, optional confirmation (pref, default on) (M5)
- [ ] Copy image to clipboard `Ctrl+C`, copy path `Ctrl+Shift+C` (`+`) (M5)
- [ ] Set as wallpaper `Ctrl+F8` (GNOME, KDE, XFCE, MATE, Cinnamon, LXDE, Fluxbox/Nitrogen/feh fallback) (M5)
- [ ] Open with… / open containing folder (M5)
- [ ] Crop — **non-goal** (editing beyond rotate/flip)

## UI
- [ ] Toggleable toolbar, status bar, scrollbars; menu via header-bar menu button (M2/M6)
- [ ] Right-click context menu (M5)
- [ ] Dark/light theme following the system (M6)
- [ ] Background colour pref ("dark background" in Viewnior) + optional checkerboard for transparency (M1/M6)
- [ ] Remember window size/position(*), maximised state, toolbar visibility, last zoom (M6)
- [ ] Preferences dialog (M6)
- [x] Error message area for unreadable files (M2)

(*) Wayland does not let clients position windows; position is restored on X11 only.

## Viewnior preferences to mirror
`zoom-mode`, `fit-on-fullscreen`, `show-hidden`, `dark-background`, `smooth-images`,
`confirm-delete`, `reload-on-save`, `show-toolbar`, `show-scrollbar`, `show-statusbar`,
`start-maximized`, `slideshow-timeout`, `auto-resize`, `behavior-wheel`, `behavior-click`,
`behavior-modify`, `jpeg-quality`, `png-compression`, `desktop`.
