# ADR-0006B: Hamix ABI v1 -- стабільний нативний ABI

Status: accepted · Date: 2026-09-27 · Plan: §5 · Reference: `docs/reference/hamix-abi.md`

## Контекст
Нативна особистість має ~90 викликів `SYS_HAMIX_*`/`SYS_NET_*`/`SYS_GPU_*`/`SYS_AUDIO_*`
(номери 9000+) плюс базові виклики з Linux-номерами (`read`, `write`, `open`, `mmap`...),
без документа і без версії. Частина з них -- не функції ядра (`CMD_*` -- реєстр команд hsh,
`MKFS`, `AUTH`, `SET_OWN_PASSWORD`).

## Рішення
* Hamix ABI v1 = таблиця в `docs/reference/hamix-abi.md`; з неї генерується
  `sdk/hamix_abi/hamix_abi.h` (C) і `userland/libs/hxlibc-sys/src/numbers.rs` (Rust) --
  `cargo xtask abi` перевіряє, що ядро, заголовок і Rust не роз'їхались.
* Версія ABI записана в ELF-ноті `.note.hamix` (name `Hamix`, type `0x48584F53`,
  desc = версія, u32). Ядро приймає версії `HAMIX_ABI_MIN..=HAMIX_ABI_VERSION`
  і відмовляє у `execve` для новіших (`ENOEXEC`). Застарілі виклики працюють щонайменше один реліз.
* Не-syscall-и виносяться в userland: `CMD_*` -> файл-реєстр `/run/hamix/commands`
  (hsh), `MKFS` -> утиліта `mkfs.hext` поверх `DISK_IO`, `AUTH`/`SET_OWN_PASSWORD` -> сервіс
  `hx-authd` (порт сервісу `hamix.auth`). До видалення з ядра вони позначені `deprecated` у таблиці.
* Нові нативні виклики (W3-W8): `FUTEX_WAIT/WAKE`, file-backed `MMAP`, універсальний
  `WAIT_ANY` (fd, порти, таймери, сигнали), `MEMFD` (іменований SHM), `DUP/FCNTL`,
  `CLOCK_*`, `GETRANDOM`, `THREAD_*`.
* IPC-модель -- порти й сервіси (`SERVICE_REGISTER/LOOKUP`, `MSG_*`, `MAILBOX_FD`) --
  лишається головною відмінністю від Linux.
* Правило: нова нативна можливість = виклик у ABI v1.x + функція в hxlibc.

## Наслідки
* hxlibc (ADR-0005B) має стабільну ціль; документ -- єдине джерело істини.
* Ядро тримає дві таблиці сумісності (стара й нова) один реліз.
