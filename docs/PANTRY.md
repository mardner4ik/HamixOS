# Pantry, the HamixOS package manager

`pantry` installs software on HamixOS. For now all packages come from
**Alpine Linux** (musl-based x86_64 builds) and run through hxlinuxulator
(see `docs/LINUXULATOR.md`). Repositories with native HamixOS software are
planned: the configuration format already knows about them, but Pantry
skips them for now.

## Commands

```
pantry update                  download and verify fresh package lists
pantry search <words>          search names and descriptions
pantry info <package>          details, dependencies, install status
pantry install <package>...    install packages plus everything they need
pantry remove <package>...     remove packages plus dependencies nothing needs any more
pantry upgrade                 upgrade every installed package
pantry repair                  download and reinstall every installed package
pantry triggers                re-run the post-install triggers (icon, pixbuf, mime caches)
pantry list [--available]      installed packages (or everything available)
pantry files <package>         files installed by a package
pantry owner <path>            which package a file belongs to
pantry repos                   configured repositories
```

`update`, `install`, `remove`, `upgrade`, `repair` and `triggers` need root
(`sudo pantry ...`).
The other commands work for any user.

## Where things live

| path | purpose |
|------|---------|
| `/etc/pantry/repositories` | one repository per line: `<alpine\|hamix> <name> <url>` |
| `/etc/pantry/keys/*.pub` | trusted RSA keys (Alpine's x86_64 signing keys) |
| `/etc/pantry/world` | packages you asked for explicitly |
| `/etc/pantry/provided` | packages HamixOS itself ships (`musl`, `busybox`, `busybox-binsh`) |
| `/var/cache/pantry/<repo>/APKINDEX` | verified package lists |
| `/var/lib/pantry/installed` | installed packages and their files |
| `/opt/linux` | where Alpine packages are unpacked (the Linux sysroot) |

The default repositories are Alpine v3.22 `main` and `community`, fetched
over plain HTTP from `dl-cdn.alpinelinux.org`. HamixOS has no TLS yet, so
every download is authenticated by signatures and hashes instead:

* **Package lists.** An `APKINDEX.tar.gz` is accepted only if its
  `.SIGN.RSA.*` (RSA/SHA-1) or `.SIGN.RSA256.*` signature verifies against
  a key in `/etc/pantry/keys`. The signed data is the raw second gzip
  member, the same scheme `apk` uses. The keys come from Alpine's
  `alpine-keys` package; I checked them against the copies in the aports
  git repository.
* **Packages.** The SHA-1 of the package's control section must match the
  `C:` checksum in the signed index. The SHA-256 of the data section must
  match the `datahash` inside that control section. Otherwise the package
  is refused before any file is written.

## Installing

* **Dependency resolution.** Pantry resolves `D:` dependencies by package
  name or by anything a package provides (`so:`, `cmd:`, `pc:` and paths
  such as `/bin/sh`).
  * Version constraints (`=`, `>=`, `<`, `~`, ...) use Alpine's version
    ordering.
  * When several packages provide the same thing, Pantry picks the one
    with the highest provider priority (`k:`), then the newest version.
  * `install_if` packages are added automatically once all their
    conditions are met by the plan.
* **Conflicts.** A file already owned by another package stops the
  install.
* **Protected files.** Pantry never overwrites the HamixOS versions of
  `etc/passwd`, `etc/shadow`, `etc/group`, `etc/hostname` and `etc/hosts`
  inside the sysroot.
* **Symlinks.** Symlinks inside packages become link files (see the next
  section). Hard links become copies.
* **Package scripts.** `.pre-install`, `.post-install`, `.pre-upgrade`,
  `.post-upgrade`, `.pre-deinstall` and `.post-deinstall` run with BusyBox
  `sh` from the sysroot, with the same arguments `apk` passes. The scripts
  are kept in `/var/lib/pantry/scripts/<package>/`.
* **Triggers.** A package's `triggers = ...` globs are stored in the
  database. After an install, upgrade or removal Pantry runs each
  `.trigger` whose globs match a directory that changed, with the matching
  directories as arguments -- this is what runs `fc-cache`,
  `update-mime-database`, `gdk-pixbuf-query-loaders`,
  `glib-compile-schemas` and `gtk-update-icon-cache`, which GTK programs
  such as `xfce4-terminal` need.
* **Toolkit extras.** `libreoffice-common` brings `libreoffice-gtk` (GTK3 on
  Wayland instead of the old X11 `gen` look), `qt5-qtbase`/`qt6-qtbase`
  bring their Wayland plugins, `gtk+3.0` brings `adwaita-icon-theme`.
* **Recommended fonts.** When a plan installs `fontconfig` and no `font-*`
  package is present, Pantry adds `font-dejavu`, so terminals like `foot`
  have something to draw with. Packages installed with an older Pantry
  never ran their scripts: remove and install them again.
* **Removal.** Removing a package also removes dependencies that nothing
  else needs (and that you did not ask for). A package another package
  still needs cannot be removed.
* **System packages.** The packages in `/etc/pantry/provided` count as
  installed. They are never downloaded, upgraded or removed, so Pantry
  cannot replace the dynamic linker HamixOS relies on.

* **Streaming.** A package is downloaded into memory once (the buffer is
  sized from `Content-Length`), then its data section is decompressed with a
  streaming inflater (32 KiB window) and unpacked by a streaming tar reader
  twice: once to list the files for the conflict check, once to write them
  in chunks. Nothing holds the whole unpacked package any more, which is why
  `libreoffice-common` (146 MiB compressed, 324 MiB unpacked) used to crash
  Pantry and now installs.
* **Mutually exclusive providers.** Two packages that provide the same
  virtual name are alternatives, not co-installable: `icu-data-en` and
  `icu-data-full` both provide `icu-data`, and both ship
  `/usr/share/icu/76.1/icudt76l.dat`. Installing one replaces the other --
  Pantry uninstalls the old provider first (running its deinstall scripts
  and removing its files) and prints `replacing icu-data-en 76.1-r1 with
  icu-data-full`. Before this, the install aborted partway through a large
  transaction with `file /usr/share/icu/... already belongs to icu-data-en`.
  Only plain virtual names count here; `so:`, `cmd:` and `pc:` names do not
  trigger a replacement on their own.
* **File takeovers.** When a package ships a file another package owns and
  the two are not exclusive providers, Pantry consults `replaces` from the
  package's `.PKGINFO` (a dependency expression, so `replaces = icu-data<71.1-r1`
  is version-checked against what the current owner provides). If the new
  package may take the file, ownership moves and Pantry prints
  `taking over N file(s) from <owner>`. A conflict that neither rule covers
  is still a hard error -- Pantry does not silently overwrite.
* **Xwayland.** If a planned package links `libX11`/`libxcb` without also
  linking a Wayland toolkit (GTK 3/4, SDL2, libwayland-client), Pantry adds
  `xwayland`; installing `xterm` adds `font-misc-misc`.
* **`pantry triggers`** re-runs every package's post-install trigger over the
  directories it owns -- the scripts that rebuild the icon caches
  (`gtk-update-icon-cache`), the gdk-pixbuf loader cache, the mime database
  and the GSettings schemas. Use it on systems installed with HamixOS 0.5.4
  or older: `sudo` gave Linux programs only the effective uid, shells dropped
  the privileges again, and those triggers failed with "Permission denied",
  which left GTK programs without icons (LibreOffice and GTK menus died on
  `Gtk:ERROR ensure_surface_for_gicon`). The bug is fixed; this command
  repairs the caches without downloading anything.
* **`pantry repair`** reinstalls every package. Use it on systems installed
  with HamixOS 0.5.3 or older: the installer's `cp -a` truncated link files
  (see below), so no installed Linux program could find its libraries.

## Symlinks in the sysroot

The HamixOS VFS has no symbolic links, and Alpine packages are full of them
(`libfoo.so.1 -> libfoo.so.1.2.3`). Pantry stores each link as a small
regular file: the bytes `\x7fHXLINK\n` followed by the target.

* The kernel follows these files for Linux tasks during path lookup, in
  the middle of a path and at the end (`open`, `stat`, `access`, `exec`
  and the ELF interpreter). A link's absolute target is taken relative to
  `/opt/linux` when the link itself lives there.
* `lstat` reports such a file as `S_IFLNK`, `readlink` returns its target,
  and `getdents` lists it as `DT_LNK`.
* `exec` follows links for every task, and hsh treats link files as
  executables.
* Native code can use `hamix_std::fs::follow_links` and
  `fs::read_following`.

## Nook integration

The applications menu is now built from desktop entries at runtime:

* `/usr/share/applications/*.desktop` holds the native apps (shipped in
  `overlay/`, with `X-Nook-Key`, `X-Nook-Order` and `X-Nook-LiveOnly`).
* `/opt/linux/usr/share/applications/*.desktop` holds apps installed by
  Pantry.

Nook rescans both directories every time the launcher opens. New apps
appear, removed ones disappear (and are dropped from the dock and the
desktop). The launcher scrolls with the mouse wheel when there are more
apps than fit. Search also matches `Keywords`, `GenericName` and
`Categories`.

* **Icons for Linux apps.** Nook looks for PNG files in
  `/opt/linux/usr/share/icons/<theme>/<size>/apps/` (hicolor first, 48x48
  preferred, any size is scaled), then in
  `/opt/linux/usr/share/pixmaps`. An app with only SVG or XPM icons gets
  a generic "linux-app" icon.
* **Terminal apps** (`Terminal=true`) open in the Nook terminal.
* **Graphical Linux apps** open their windows through the Wayland bridge
  `hxwayland` (see `docs/LINUXULATOR.md`). Software-rendered (`wl_shm`)
  clients work; clients that need EGL/GPU buffers do not.

Tested in QEMU:

1. `pantry update` verified both Alpine indexes (5647 + 20678 packages).
2. `pantry install htop` resolved `libncursesw` and `ncurses-terminfo-base`
   and installed them.
3. `htop` runs on the console and in the Nook terminal.
4. After the install, Htop appeared in the launcher with its own icon and
   started from there.
5. After `pantry remove htop`, it disappeared from the menu.

## Not done yet

* Native HamixOS repositories (`hamix` lines in `repositories`).
* HTTPS: downloads are HTTP-only and rely on the signatures and hashes
  above.
* Resuming interrupted downloads and caching `.apk` files.
