# Розкладка файлової системи HamixOS

Status: current · Updated: 2026-09-28 · ADR: `docs/adr/0013B-linux-environment-and-live-root.md`

Два світи, одне дерево: нативна система HamixOS займає звичні каталоги Unix, а весь
софт Alpine Linux живе окремо в `/linux` і не змішується з нативним.

## Нативна система

| Шлях | Що там |
|---|---|
| `/boot` | ядро `kernel.bin`, ранні модулі `kmods.tar`, конфіг GRUB (на встановленій системі) |
| `/bin` | лише `sh` і `bash` (заглушка, що запускає `/usr/bin/bash`; `sh` -- POSIX-режим) |
| `/lib` | `ld.hx` -- динамічний завантажувач hxlibc; `/lib/modules` -- модулі драйверів |
| `/usr/bin` | програми й команди для людей: Nook-програми, команди HamixOS (`sudo`, `fetch`, `lsblk`, `linux`...), bash, busybox-утиліти, порти (`curl`, `lua`, `dbus-send`...) |
| `/usr/libexec/hamix` | служби й внутрішні помічники, які не запускають руками: `hxsvc` (PID 1), `hxlogin`, `hx-authd`, `hxwayland`, адаптери Linux-шару (`hx-login1`, `hx-upower`, `hx-polkit`, `hxsvc-openrc`), перевірки `hxproto-check`, `wl-layer-check` |
| `/usr/lib` | `libhxc.so` і бібліотеки портів (`.so`), `hamix/` -- тести, бенчмарки, образи GRUB |
| `/usr/share` | Nook (`nook/`, значки, теми, шпалери), шрифти, `applications/*.desktop`, `hamix/install.sh` |
| `/etc` | конфігурація: `passwd`/`group`/`shadow`/`sudoers`, `profile`, `bash.bashrc`, `hamix/` (служби `services/*.svc`, `login.conf`, `linux.conf`), `pantry/`, `fonts/`, `dbus-1/` |
| `/var` | змінні дані: `lib/hamix/commands` (реєстр `cmd`, обгортки в `bin/`), `lib/pantry`, `cache`, `log/hx` |
| `/home`, `/root` | домашні каталоги |
| `/tmp`, `/run`, `/dev`, `/proc`, `/sys`, `/mnt`, `/media` | як у Unix; не пишуться на диск |

`PATH=/usr/bin:/bin:/usr/sbin:/sbin:/var/lib/hamix/commands/bin`.

## Linux-оточення: `/linux`

Корінь Alpine: `/linux/bin` (busybox), `/linux/lib/ld-musl-x86_64.so.1`, `/linux/usr`,
`/linux/etc`, `/linux/var`... Сюди `pantry` ставить пакунки Alpine; нативний `/usr` вони
не чіпають.

Процес із Linux-особистістю бачить файлову систему так:

| Linux-процес бачить | Що це насправді |
|---|---|
| `/`, `/usr`, `/etc`, `/bin`, `/lib`, `/var`... | `/linux/...` |
| `/home`, `/root`, `/tmp`, `/run`, `/dev`, `/proc`, `/sys`, `/mnt`, `/media` | ті самі каталоги, що й у HamixOS (спільні) |
| `/etc/passwd`, `group`, `shadow`, `hostname`, `hosts`, `resolv.conf`, `localtime`, `timezone`, `machine-id`, `sudoers`, `/etc/hamix` | нативні файли (одні користувачі, одна мережа) |
| `/hamix/...` | нативне дерево HamixOS (`/hamix/usr/bin/bash` = `/usr/bin/bash`) |

Нативні програми бачать оточення як звичайний каталог `/linux`. Абсолютні посилання
всередині `/linux` тлумачаться в поданні Linux (`/linux/usr/lib/libfoo.so -> /usr/lib/...`
веде в `/linux/usr/lib`), скрипти з `/linux` отримують свій `#!/bin/sh` з `/linux/bin`.
`/linux/usr/bin/bash` посилається на `/hamix/usr/bin/bash`, тому термінали й скрипти
Linux-програм користуються нативним bash.

Запуск: `linux КОМАНДА` (або просто `linux` -- оболонка оточення); Linux-програми з
`.desktop` у `/linux/usr/share/applications` з'являються в меню Nook разом із
нативними. Вимкнення шару (`hxctl linux off`, `linux=0`, профіль `core`) не зачіпає
нативну систему: у профілі `core` каталогу `/linux` немає взагалі.

## Жива система

ISO містить образ кореня `/boot/hext.img`. Завантажене з CD/DVD (реального чи
віртуального), ядро читає цей образ прямо з диска на вимогу -- у пам'яті лише файли, які
справді відкрили, а зміни лишаються в ОЗП. Пункт меню "copy the whole system into RAM"
(`root=ram`) і завантаження з USB вантажать образ у пам'ять повністю.
