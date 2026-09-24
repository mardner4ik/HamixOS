# hsh — the HamixOS shell

hsh is an ordinary userspace program (`/usr/bin/hsh`, source in `apps/hsh`).
The kernel only handles the console login.

## Which shell starts after login

`/etc/hamix/login.conf`:

```
shell=/usr/bin/hsh          # any static HamixOS program
shell_args=-l
desktop=/usr/bin/hxserver   # used by startx and autostart
autostart_desktop=no        # yes = start Nook right after login
```

The file is read at every login. If the configured shell does not exist the
login prompt says so and returns. `session autostart on|off` changes the
autostart setting (root).

## Invocation

```
hsh                 interactive (history in ~/.hsh_history, ~/.hshrc, /etc/hshrc)
hsh -l              login shell (also prints /etc/motd)
hsh -c 'commands'   run a command string
hsh script.sh args  run a script; ./script.sh works with #!/usr/bin/hsh
                    or when the file ends in .sh
```

## Language

* `a; b`, `a && b`, `a || b`, `! a`, `a | b | c`, `cmd &`
* redirections `>`, `>>`, `<`, `2>`, `2>>`, `2>&1`, `&>`
* quoting `'…'`, `"…"`, `\x`; variables `$VAR`, `${VAR}`, `${VAR:-default}`,
  `${#VAR}`, `$1…$9`, `$@`, `$#`, `$?`, `$$`, `$!`; `$(command)`; `~`; globs
  `*` and `?`
* `if …; then …; elif …; else …; fi`, `while`/`until … do … done`,
  `for x in …; do … done`, `name() { … }`, `break`, `continue`, `return`
* `set -e`, `set -x`

## Line editing

Arrows, Home/End, Ctrl+A/E/K/U/W/L, history with Up/Down, Tab completion of
commands and paths (twice to list), Ctrl+C cancels the line or stops the
running program, Ctrl+D on an empty line exits.

## Builtins

`help` lists them and `help <name>` shows usage. Groups:

* shell: cd pwd echo printf exit export unset set source sh alias history
  type which test [ read shift sleep clear exec wait jobs env eval loadconf setconf
* files and text: ls cp mv rm mkdir rmdir touch chmod chown tree stat find du
  basename dirname realpath cat grep head tail wc sort uniq tee xxd write
* system: uname whoami id hostname date uptime free ps kill dmesg cpuinfo
  lsusb gpuinfo mouse drivers version fetch sync reboot halt poweroff cmd
  startx edit session
* users: sudo su passwd useradd userdel usermod chpasswd users
* disks: diskls (lsblk) blkid diskpart mkfs.hext mount umount df bootinstall

Commands that are not builtins are searched in `$PATH`
(`/usr/bin:/bin:/sbin:/usr/sbin`) and then in the command registry.

## Privileges

`sudo` asks for your password; if you are listed in `/etc/sudoers` the kernel
gives the shell a five-minute ticket and the command runs with uid 0. `su`
asks for the root password.
