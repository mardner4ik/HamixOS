# ADR-0012B: bash на hxlibc -- системна оболонка, команди -- окремі програми

Status: accepted · Date: 2026-09-28 · Plan: §4A.7, §9

## Контекст
`hsh` -- власна оболонка (5,5 тис. рядків), у якій жили й оболонка, і ~110 вбудованих
команд (`sudo`, `fetch`, `diskls`, `mount`, `ls`, `grep` ...). Будь-яка інша оболонка чи
скрипт не бачили цих команд, `sudo` працював лише всередині hsh, а синтаксис був
лише частково POSIX. hxlibc уже тримає busybox (щабель 1 драбини портів), тож
справжня оболонка стала можливою нативно, без Linux-шару.

## Рішення
* Системна оболонка -- **GNU bash 5.2** з `ports/core/bash`, зібраний `hx-cc` проти hxlibc
  (нативна особистість, працює і в профілі `core`). `/bin/sh` і `/bin/bash` -- маленька
  нативна заглушка `sh` з `hxutils`, яка `execve`-ить `/usr/bin/bash` зі своїм `argv[0]`
  (`sh` вмикає POSIX-режим); копії bash не потрібні, бо у hext немає символьних посилань.
* `hsh` **вимкнено, не видалено**: `[package.metadata.hamix] disabled = true` у його
  `Cargo.toml`; `cargo xtask build` його не ставить, `cargo xtask apps` показує як disabled.
* Команди, специфічні для HamixOS, -- окремі програми крейта `userland/core/hxutils`
  (одна бібліотека + `src/bin/*.rs`, кожен файл -- окремий ELF у `/usr/bin`): `sudo`, `su`,
  `passwd`, `useradd`, `userdel`, `usermod`, `chpasswd`, `users`, `fetch`, `gpuinfo`, `cpuinfo`,
  `startx`, `session`, `edit`, `cmd`, `diskls`/`lsblk`, `blkid`, `diskpart`, `mkfs`/`mkfs.hext`,
  `mount`, `umount`, `df`, `bootinstall`, `ip`/`ifconfig`, `netmode`, `netconf`, `ping`,
  `nslookup`, `wifi`, `wget`, `uname`, `id`, `whoami`, `hostname`, `date`, `uptime`, `free`, `ps`,
  `kill`, `dmesg`, `lsusb`, `mouse`, `drivers`, `modules`, `modinfo`, `interrupts`, `version`,
  `sync`, `reboot`, `halt`, `poweroff`, `syscalltrace`, `tree`, `setconf`, `hxconf`.
* Загальні утиліти (`ls`, `cat`, `grep`, `sed`, `awk`, `find`, `tar`, `vi`, `less` ...) -- busybox
  на hxlibc через обгортки `#!/usr/bin/busybox` у `/usr/bin`.
* `sudo` -- окрема програма: автентифікація через `hx-authd`, який пам'ятає успішний вхід
  5 хвилин для пари (користувач, термінал); `sudo -k` забуває, `-v` поновлює, `-i`/`-s` --
  оболонка root.
* Ядро отримало POSIX-групи переднього плану: `TIOCSPGRP`/`TIOCGPGRP` на VT і на pty.
  Поки VT належить POSIX-оболонці, клавіатура й Ctrl+C ідуть групі, яку оболонка
  зробила активною; коли VT забрала графічна програма (hxserver), діє старе правило
  власника.
* Зареєстровані `cmd add` команди стають виконуваними обгортками в
  `/var/lib/hamix/commands/bin` (є в `PATH`), тож працюють із bash, скриптів і `sudo`.
* Завантажувальні тести -- bash-скрипти `tests/boot/*.sh`.

## Наслідки
* Будь-яка POSIX-оболонка чи програма бачить команди HamixOS як звичайні файли.
* Скрипти з Alpine/інтернету працюють без переписування; `install.sh` -- bash.
* Кожна команда -- окремий процес (40-90 КБ); fork/exec на hxlibc -- ~1 мс.
* hsh лишається в дереві для історії та можливого повернення (вимкнення -- один рядок).
