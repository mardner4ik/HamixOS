# ADR-0018B: шари як у Redox -- ядро за підсистемами, `crates/`, повне дерево hxlibc

Status: accepted · Date: 2026-10-05 · Plan: UNIX_PLAN U10 · Уточнює: ADR-0015B, ADR-0017B

## Контекст
* `kernel/src/syscall/` мав три гілки -- `posix/`, `linux/`, `hamix/` -- і в `posix/` лежали
  цілі підсистеми, що не є системними викликами: procfs (файлова система), DRM (інтерфейс GPU),
  сокети, AF_INET, netlink (мережа), tty (термінали), epoll. Linux-особистість була розкидана по
  `personality.rs`, `syscall/linux/` і `task/linux_loader.rs`.
* Бібліотеки: верхній `libs/` (hext, hxvt, wlan) і `userland/libs/` мали однакову назву, а в
  `userland/libs/` поруч лежали чисті переносні крейти (hxproto, hxtoolkit, vellum, кодеки) і
  прив'язані до ОС (hxstd, hxport).
* mlibc існувала лише в `target/`: xtask клонував upstream і накладав 23 патчі під час кожного
  чистого збирання; змінювати libc означало писати патч.

## Як це в Redox
* Ядро мінімальне: `syscall/` лише розбирає аргументи; підсистеми -- окремі модулі (`scheme/*`).
* Бібліотеки за шарами: `redox_syscall` (номери й типи) -> `libredox` (зручні обгортки) ->
  `relibc` (libc на Rust, через неї ходить увесь POSIX-код); незалежні крейти (`orbclient`,
  `redoxfs`) не залежать від ОС і живуть окремо.

## Рішення
* Ядро:
  * `syscall/` -- POSIX-шар: `table.rs` (номери POSIX і диспетчер), `base.rs` (файли), `process.rs`,
    `signal.rs` + `sigframe/`, `misc.rs`, `special.rs`; `ext/` -- розширення HamixOS (колишній
    `hamix/`), `core_table.rs` -- генерований реєстр ABI.
  * `linux/` -- уся Linux-особистість: `mod.rs` (вмикання/вимикання), `syscalls.rs` (нумерація
    asm-generic для aarch64/riscv64), `paths.rs` (подання `/linux`), `loader.rs` (ELF Linux).
  * `fs/procfs.rs`, `fs/epoll.rs`, `net/socket.rs`, `net/socket_inet.rs`, `net/netlink.rs`,
    `drivers/video/drm.rs`, `tty/{mod,vt,pty}.rs`.
  * Нові: `arch/x86_64/ioapic.rs`; кроки завантаження показують прогрес і сторож.
* Бібліотеки:
  * `crates/` -- переносні Rust-крейти без викликів ОС: `hext`, `hxvt`, `wlan`, `hxproto`,
    `hxtoolkit`, `vellum`, `imago`, `hx264`, `hxaac`, `hxmp4`. Їх можна збирати для ядра,
    інструментів хоста й будь-якої ОС.
  * `userland/libs/` -- бібліотеки HamixOS, що звертаються до системи: `hxstd` (≈ libredox),
    `hxport`, `hxclient`, `libhxproto` (C `.so`).
  * `hxlibc/` -- libc (≈ relibc): повне дерево mlibc 5.0.0 з уже накладеними змінами HamixOS і
    `sysdeps/hamix`; `subprojects/` (frigg, cshim, cxxshim) у дереві; meson збирає його напряму.
    `hxlibc/rust/hxlibc-sys` -- Rust-бік; сюди ж переїжджатиме Rust-код самої libc.
    `hxlibc/applied-patches/` -- історія змін для перенесення на новішу mlibc.
* Завантаження: `/boot/initfs.tar` (лише драйвери `early = 1`) замість `kmods.tar` з усіма модулями.

## Наслідки
* Новий системний виклик -- у `syscall/` (POSIX) або `syscall/ext/`; логіка -- у підсистемі.
* Новий крейт: без звернень до ОС -> `crates/`, інакше -> `userland/libs/`.
* `cargo xtask abi` читає `syscall/ext` і `syscall/table.rs`, `lint-layers` -- `kernel/src/linux`.
* Збирання hxlibc більше не потребує мережі; `libhxc.so` той самий (відрізняються лише шляхи
  `__FILE__`).
