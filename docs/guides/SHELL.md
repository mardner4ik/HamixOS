# Оболонка: bash і команди HamixOS

Status: current · Updated: 2026-09-28 · ADR: `docs/adr/0012B-bash-system-shell.md`

Системна оболонка -- GNU bash 5.2, зібраний нативно проти hxlibc (`ports/core/bash`).
Колишня оболонка hsh вимкнена (див. `HSH.md`).

## Що запускається після входу

`/etc/hamix/login.conf`:

```
shell=/usr/bin/bash
shell_args=-l
desktop=/usr/bin/hxserver   # для startx і автозапуску
autostart_desktop=no
```

Якщо вказаної оболонки немає (наприклад, на старому встановленні лишився
`/usr/bin/hsh`), `hxlogin` запускає `/usr/bin/bash -l`.

Файли запуску: `/etc/profile` (PATH, `/etc/profile.d/*.sh`, motd для login-оболонки),
`/etc/bash.bashrc` (промпт, аліаси `ll`/`la`/`l`/`..`, історія, пошук зареєстрованих
команд), далі `~/.bash_profile` / `~/.bashrc` як звичайно. `/bin/sh` -- bash у
POSIX-режимі.

`PATH=/usr/bin:/bin:/usr/sbin:/sbin:/var/lib/hamix/commands/bin`.

## Команди

Кожна команда -- окремий файл у `/usr/bin`, тож працює з будь-якої оболонки, скрипта,
`sudo`, `env`, `xargs`:

| Група | Програми (крейт `userland/core/hxutils`) |
|---|---|
| користувачі | `sudo` `su` `passwd` `useradd` `userdel` `usermod` `chpasswd` `users` `id` `whoami` |
| система | `fetch` `gpuinfo` `cpuinfo` `uname` `hostname` `date` `uptime` `free` `ps` `kill` `dmesg` `lsusb`/`usb` `mouse` `drivers` `modules` `modinfo` `interrupts` `version` `sync` `reboot` `halt` `poweroff` `syscalltrace` |
| сесія | `startx` `session` `edit` `cmd` `setconf` `hxconf` |
| диски | `lsblk`/`lsblk` `blkid` `diskpart` `mkfs`/`mkfs.hext` `mount` `umount` `df` `bootinstall` |
| мережа | `ip`/`ifconfig` `netmode` `netconf` `ping` `nslookup` `wifi` `wget` |
| файли | `tree` |

Загальні утиліти (`ls cat cp mv rm mkdir grep sed awk find tar vi less head tail sort`
...) -- busybox на hxlibc (`/usr/bin/busybox`, обгортки `#!/usr/bin/busybox`).

## sudo

```
sudo COMMAND [ARGS]     як root (пароль свого користувача, група sudo)
sudo -i | -s            оболонка root (login / звичайна)
sudo -v                 поновити 5-хвилинний дозвіл
sudo -k                 забути дозвіл
```

Пароль перевіряє `hx-authd`; після успіху він 5 хвилин не питається знову в тому
самому терміналі. `su [-] [-c CMD]` питає пароль root.

## Налаштування

`setconf FILE KEY VALUE` міняє рядок `KEY=VALUE` у конфігу (як робили `session`,
Налаштування й установник), `hxconf FILE [KEY]` читає його.
