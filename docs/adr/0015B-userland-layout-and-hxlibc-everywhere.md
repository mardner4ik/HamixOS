# ADR-0015B: усі програми в `userland/<роль>/`, Rust-програми на hxlibc

Status: accepted · Date: 2026-09-29 · Plan: §4A.4 (hxstd поверх hxlibc), §10.1 · Уточнює: ADR-0005B, ADR-0010B

## Контекст
* Власні програми лежали в `userland/{apps,core,desktop,services}`, а зібрані на hxlibc
  bash, busybox, dbus, glib, GTK 3 -- окремо в `hx-ports/`; Rust-бібліотеки userland --
  в `libs/` разом із крейтами ядра. Де шукати програму, залежало від того, хто її написав.
* План B (§4A.4) обіцяв `hxstd` поверх hxlibc, але Rust-програми були статичними
  ELF із власним `_start`, купою й сирими викликами; hxlibc у них не було взагалі.

## Рішення
* **Розкладка за роллю:** `userland/apps` (програми з вікнами), `userland/core` (команди
  й оболонки: hxutils, pantry, bash, busybox, curl, lua, sqlite), `userland/desktop`
  (hxserver, hxsettings, hxwayland), `userland/services` (hxsvc, hx-authd, hxsound,
  dbus), `userland/libs` (Rust: hxstd, hxclient, hxproto, vellum, кодеки; C: glib, cairo,
  pango, gtk3, ...). Каталог із `Cargo.toml` -- крейт, із `HXBUILD` -- порт.
  `libs/` лишився для крейтів ядра й інструментів хоста (`hext`, `hxvt`, `smoltcp`,
  `wlan`). `hx-ports/README.md` -> `docs/guides/PORTS.md`.
* `cargo xtask` сам дописує каталоги портів у `exclude` робочого простору Cargo
  (`userland/*/*` без `Cargo.toml`) і шукає рецепти в `userland/*/*/HXBUILD`;
  `cargo xtask apps` показує крейти й порти разом.
* **Кожна нативна програма -- програма hxlibc.** xtask генерує ціль
  `target/specs/x86_64-hamix.json` (`os = "hamix"`, PIC, PIE, `/lib/ld.hx`, `Scrt1.o`,
  `-lhxc`); Rust-програми стають динамічними, як C-програми з `hx-cc`, і мають ноту ABI
  v1 (POSIX-семантика). `hxstd` на `target_os = "hamix"`: вхід -- `main(argc, argv,
  envp)` від `__mlibc_entry`, `exit` -- через libc (atexit, stdio), `SIGPIPE`
  ігнорується, купа -- `malloc`/`free` (великі блоки -- прямо `mmap`, бо `calloc` у
  mlibc заповнює нулями), `read`/`write`/`open`/`close`/`dup`/`lseek` -- функції libc.
  `hxlibc-sys` -- справжні FFI-прив'язки до `libhxc.so` (модуль `libc`) плюс номери
  викликів для нативних `SYS_HAMIX_*`.
* aarch64 і riscv64 лишаються на статичному `link.ld`, доки для них не зібрано hxlibc
  (потрібні UAPI-заголовки Linux для цих архітектур); `hxstd` підтримує обидва режими.
* **Пам'ять:** frigg (алокатор mlibc) розбивав кожен новий slab на 256 КіБ одразу,
  торкаючись усіх сторінок, -- 7 класів розміру × 2 (ld.hx і libc) ≈ 3.5 МіБ на процес.
  Патч `0011-frigg-lazy-slab-partition` ділить slab посторінково. Мінімальний процес
  hxlibc: 4 МіБ -> 1.1 МіБ; система: консоль 19 МіБ (було 22), Nook 35 МіБ (було 36).

## Наслідки
* Одна libc на всю систему: Rust-програми можуть напряму викликати C-бібліотеки
  (cairo, freetype, GTK) без окремого рантайму; bash, busybox і GTK-програми дешевші.
* Тест `native-memory`: процес hxlibc стартує < 2 МіБ, служби < 3 МіБ.
* Далі: решта функцій `hxstd` (fs, net, unix) -- поступово через libc; hxlibc для
  aarch64/riscv64.
