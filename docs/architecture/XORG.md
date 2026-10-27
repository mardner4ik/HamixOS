# hxserver: the Nook display server

`apps/hxserver` is HamixOS's display server and the Nook desktop. It is an
ordinary ring-3 program (`/usr/bin/hxserver`, started with `startx`); the
kernel only provides generic building blocks that any process may use:

| Building block | Syscalls | Kernel code |
|----------------|----------|-------------|
| Framebuffer mapping (write-combining, 4K pages in the caller's address space) | `hamix_fbmap` 9001, `hamix_release_fb` 9006 | `task/display.rs` |
| Named services | `service_register` 9022, `service_lookup` 9023 | `task/ipc.rs` |
| Messages (≤ 4096 bytes, per-process mailbox) | `msg_send` 9020, `msg_recv` 9021 | `task/ipc.rs` |
| Shared memory segments | `shm_create` 9030, `shm_map` 9031, `shm_release` 9032 | `task/ipc.rs` |
| Sleeping until input or a message arrives | `wait_event` 9014 | `syscall/mod.rs` |

When you switch to another virtual terminal (Ctrl+Alt+F2..F6) the kernel
copies the screen into a RAM shadow and remaps the server's framebuffer
pages onto it, so the desktop keeps running in the background and is put
back exactly as it was when you return.

## Window protocol (`libs/hxproto`)

hxserver registers the service name `hxserver`. Messages are small
little-endian records; the first `u32` is the message kind.

Client → server:

| Request | Fields |
|---------|--------|
| `CreateWindow` | shm id, width, height, title (48 bytes) |
| `Present` | window, damaged rectangle x, y, width, height |
| `SetTitle` | window, title |
| `SetCursor` | window, cursor shape |
| `SetIcon` | window, icon name (used by the dock and Alt+Tab) |
| `SizeHints` | window, min width/height, max width/height (0 = no limit; min = max makes the window fixed-size) |
| `Attach` | window, shm id, width, height -- the client's answer to `Resize` |
| `Notify` | title, body -- a desktop notification |
| `Reload` | re-read `~/.config/nook.conf` (wallpaper, dock) |
| `DisplayChanged` | the screen mode was changed; the server remaps the framebuffer |
| `ChooseFile` | request id, mode (open file / folder / save file), title, filters, start path -- see "File chooser" |
| `SetMaximized` | window, 0/1 -- maximize or restore the client's own window |
| `Destroy` | window |

Server → client:

| Event | Fields |
|-------|--------|
| `Created` | window id |
| `Mouse` | window, x, y (content-relative), buttons, kind (move/press/release/wheel/leave), wheel |
| `Key` | window, key code (same codes as `hamix_pollkey`) |
| `Focus` | window, focused |
| `Resize` | window, new content width and height |
| `Close` | window -- the user closed it; the server has already removed it |
| `FileChosen` | request id, status (0 chosen, 1 cancelled, 2 failed), path |
| `Rejected` | reason |

Titles are at most 110 bytes; paths and filter lists use a longer text field
(1024 bytes), so a message can be up to 2400 bytes.

## File chooser

Programs never browse the filesystem themselves to let the user pick a file.
They ask Nook, which opens a Files window in picker mode -- the same idea as
the XDG desktop portal on Linux or the common file dialog on Windows:

```rust
let path = window.open_file_dialog("Open a video", "Videos|mp4,m4v,mov;All files|*", "/home/user/Videos");
let folder = window.choose_folder_dialog("Choose a folder", "/home/user");
let target = window.save_file_dialog("Save the note as", "Text files|txt,md", "/home/user/Documents/note.txt");
```

These helpers block until the user answers and return `None` when the dialog
was cancelled; other events that arrive meanwhile are kept for `wait_event`.
Programs that must keep working while the dialog is open (a video player, for
example) call `window.request_file(mode, title, filters, start)` and handle
`Event::FileChosen` themselves.

* **Filters** are `Name|ext,ext;Name|ext`. `*` matches every file. An
  "All files" entry is added when the list has none. The picker shows the
  first filter; the filter button cycles through them. Folders are always
  shown so the user can navigate.
* **Modes**: open file (double click or Open), choose folder (the selected
  folder, or the folder that is open), save file (a file name field; the start
  path may include a suggested name).
* **Flow**: the client sends `ChooseFile`; hxserver starts
  `/usr/bin/hxfiles --pick=open|folder|save --token=N --title=… --filter=…
  --start=…` and remembers the token, the picker's pid and the client. The
  picker answers with `PickerResult { token, status, path }`; hxserver only
  accepts it from the process it started with that token and forwards
  `FileChosen` to the client. If the picker exits without answering the client
  gets "cancelled"; if the client exits the picker is closed.
* Files opens videos in Videos and `.wav` files with `hxsound`; Notes uses the
  open and save dialogs.

A window's pixels live in a shared memory segment the client creates
(`width * height` × `0x00RRGGBB` u32). The client draws into it and sends
`Present` with the changed rectangle; the server composites only that area.

## Writing a client (`libs/hxclient`)

```rust
use hxclient::{ui, Event, Window};

let mut window = Window::open("Demo", 320, 200)?;
ui::rect(&mut window, 0, 0, 320, 200, ui::BG);
ui::text(&mut window, 16, 16, "hello", ui::TEXT);
window.present();
while let Some(event) = window.wait_event(-1) {
    if let Event::Close { .. } = event { break; }
}
```

`Window` implements `vellum::Canvas`, coalesces mouse-move and resize events
and turns the death of the server into a `Close` event. Examples:
`apps/hxhello`, `apps/hxmon` (system monitor), `apps/hxfiles` (file browser)
and `apps/hxcmd` (command registry).

## Wayland clients

`apps/hxwayland` translates the Wayland protocol into this protocol: it is
an ordinary client that owns one Nook window per `xdg_toplevel`, copies the
client's `wl_shm` buffer into the window on every commit and turns Nook's
mouse, key, focus, resize and close events into Wayland events. hxserver
starts it at startup. Details are in `docs/LINUXULATOR.md` ("Wayland
bridge").

The `Key` event now also carries `mods` (bit 0 Shift, bit 1 Alt, bit 2
Ctrl), and F1-F12 and Insert have key codes (-41..-52 and -53).

## Resizing

Windows can be resized from any edge or corner, maximized (title-bar button or
double click), snapped to the left/right half by dragging them to the screen
edge, or maximized by dragging them to the top. The flow is:

1. the server decides the new content size and sends `Resize`;
2. `hxclient` reuses the shared memory segment when it is large enough,
   otherwise it creates a new one (rounded up to 256 px steps so dragging does
   not allocate on every pixel) and sends `Attach`;
3. the application receives `Event::Resize`, lays itself out again using
   `window.size()` or `hxclient::window_width()` / `window_height()` and
   presents a full frame. Until then the server keeps showing the old pixels.

Applications that cannot shrink below a certain size send `SizeHints`
(`window.set_min_size(w, h)`); the window manager respects them when resizing
and snapping.

## Desktop

* Dock: pinned applications first, then running ones. Right click → pin/unpin,
  drag to reorder; the order is saved as `dock=` in `~/.config/nook.conf`.
  The dock slides away when a window overlaps it and comes back when the
  pointer touches the bottom edge or no window covers it any more.
* Desktop icons can be dragged anywhere; positions are stored in
  `~/.config/nook.conf`. Launcher entries can be added to the desktop.
* Top bar: application menu, calendar (click the clock), network indicator
  (Ethernet / Wi-Fi signal / offline) with a popup that lists adapters and
  Wi-Fi networks and switches between Ethernet and Wi-Fi, sound (speaker icon:
  popup with the device, volume slider and mute; scroll over it to change the
  volume; keyboard volume keys show an on-screen level above the dock -- see
  `docs/AUDIO.md`), user menu and power.
* Alt+Tab switches windows, Alt+F4 closes the focused one, Super opens the
  launcher.
* When the resolution changes (Settings → Display) the server remaps the
  framebuffer, rescales the wallpaper and moves windows back on screen.

## How the desktop stays smooth

* Damage tracking: every change (cursor, window move, client `Present`,
  clock tick) adds a rectangle; only those rectangles are recomposed in RAM
  and copied to the framebuffer, row by row with `memcpy`.
* The framebuffer is mapped write-combining through the PAT, both in the
  kernel console and in hxserver.
* The server sleeps in `wait_event` until the mouse, keyboard or a client
  wakes it, instead of spinning.
* The scaled wallpaper is cached in `/var/cache/nook/` so PNG decoding only
  happens once per resolution.
* Windows that are completely hidden behind opaque windows are skipped while
  composing; shadows are drawn only inside their real bounds.
* PNG decoding (`libs/mini_png`) inflates into a presized buffer, unfilters in
  place and converts straight to ARGB -- about five times faster than before.
