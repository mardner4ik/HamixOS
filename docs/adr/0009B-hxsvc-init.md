# ADR-0009B: hxinit у ядрі, hxsvc як PID 1 у користувацькому просторі

Status: accepted · Date: 2026-09-27 · Plan: §9

## Контекст
Уся ініціалізація й логін зараз у ядрі (`kernel/src/hxinit.rs`, `kernel/src/login.rs`):
ядро саме читає `/etc/hamix/login.conf`, питає пароль і запускає оболонку. Сервісів як
окремої сутності немає, журналу теж.

## Рішення
* `hxinit` лишається ядерним boot-етапом (пам'ять, PCI, диски, rootfs, пристрої).
* Після нього ядро запускає `/sbin/hxsvc` як процес із PID 1 (параметр `init=`,
  типово `/sbin/hxsvc`; якщо файлу немає -- стара ядерна процедура логіну як запасна).
* `hxsvc` (Rust, userland) -- декларативні сервіси `/etc/hamix/services/*.svc`:
  `exec`, `after`, `restart`, `memory_max`, `on_port` (ледачий старт за сервіс-портом),
  `tty`. Журнал `hxlog` (кільце в пам'яті + `/var/log/hx/*.log`). CLI `hxctl`
  (`status`, `start`, `stop`, `restart`, `log`, `linux on|off|status`).
* Логін -- сервіс `getty` (`hxlogin`) на tty, не код ядра.
* Linux-демони (OpenRC-скрипти) -- лише через адаптер `hxsvc-openrc` у Linux-шарі;
  `org.freedesktop.login1` дає адаптер `hx-login1`, udev замінює `libudev-zero`
  (див. `docs/guides/SERVICES.md`, "Адаптери Linux-шару").

## Наслідки
* Ядро меншає і не парсить конфіги користувача.
* Порядок старту, перезапуск, ліміти пам'яті й журнал -- в одному місці.
