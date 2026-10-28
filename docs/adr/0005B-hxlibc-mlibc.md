# ADR-0005B: hxlibc -- власна libc як форк mlibc

Status: accepted · Date: 2026-09-27 · Plan: `docs/roadmap/STRATEGY_2026_B.md` §4, §4A

## Контекст
Нативні програми HamixOS зараз лінкуються з `hamix_std` (~3300 рядків Rust, `no_std`),
який дає лише Rust-API. C/C++-софт зібрати під нативну особистість неможливо; весь
чужий софт іде через Linux-шар і musl. Мета плану B -- одна власна libc для нативного
світу і, поступово, для портованого софту (режим 2, §4A.3).

## Рішення
* hxlibc = форк **mlibc** (пінований реліз), каталог `hxlibc/`:
  `vendor/` -- апстрім без змін, `sysdeps/hamix/` і `abis/hamix/` -- наш код.
  Збірка Meson, обгортка `cargo xtask hxlibc-build`.
* Бекенд `sysdeps/hamix` робить виклики **Hamix ABI v1** (ADR-0006B); розкладка
  структур (`struct stat`, `sockaddr`, `dirent`, сигнали) -- Linux-подібна, номери викликів власні.
* Toolchain `hx-cc`: clang + lld, `--target=x86_64-unknown-hamix`, власний sysroot,
  `crt0/crti/crtn`; Meson cross-file на кожну архітектуру.
* Rust: `userland/libs/hxlibc-sys` -- ті самі виклики для `no_std` Rust; `userland/libs/hxstd` -- facade
  зі старим API `hamix_std` поверх `hxlibc-sys`. Єдине джерело номерів -- `docs/reference/hamix-abi.md`
  і згенерований з нього `hamix_abi.h`.
* Спайк W1-W2 з критерієм go/no-go (§4A.6).

## Наслідки
* Ядро libc -- C/C++; "Rust-ідентичність" переноситься на ядро, hxserver, Nook, hxsvc, `hxlibc-sys`.
* Готові Alpine-бінарники без musl не працюють (сумісність mlibc -- на рівні вихідного коду),
  тому Linux-шар на musl лишається у 2026 (режим 1).
* Оновлення mlibc -- зміна тегу в `hxlibc/UPSTREAM` і перевірка `patches/`; diff із апстрімом видно окремо.

## Альтернативи
* Писати libc з нуля на Rust -- роки роботи.
* relibc (Rust) -- запасний варіант, якщо спайк mlibc провалиться.
* musl для всього (план A) -- немає власної libc.

## Результат спайка (2026-09-27): go

* mlibc 5.0.0 + `hxlibc/sysdeps/hamix` (похідний від `sysdeps/linux`, бо діапазон core
  Hamix ABI має нумерацію Linux x86-64) збирається `cargo xtask hxlibc-build` за ~1 хв.
* `tests/hxlibc/spike.c` у QEMU: printf, malloc/realloc, файли й stdio, stat, 4 pthread з
  м'ютексом, fork+waitpid, fork+execv, обробник сигналу, clock_gettime, getenv -- усе проходить.
* `busybox` 1.37 (ash + ~90 аплетів, рецепт `ports/core/busybox`) збирається й проходить
  `tests/boot/native-busybox-hxlibc.sh`; бінарник 288 КБ проти 1,26 МБ musl-static
  (набір аплетів менший, порівняння часу старту -- у `kbench`).
* Спайк знайшов і виправив дві вади ядра на рівні POSIX: `open()` після `close(0)` не
  повертав найменший вільний fd; неявна консоль для закритих fd 0-2 у POSIX-процесах.

Уточнення рішень, прийняті під час спайка:
* **Динамічне лінкування -- типове:** `PT_INTERP=/lib/ld.hx`, `libhxc.so` (патч
  `hxlibc/patches/0002` дає імена, що не конфліктують із musl у спільному `/usr/lib`).
  Статичне лінкування відкладене: у ET_EXEC на базі `0x80_0000_0000` слабкі невизначені
  sysdep-символи mlibc дають переповнення PC32-релокацій; потрібен `-mcmodel=large`
  або static-pie у mlibc.
* Апстрім не зберігається в git: `hxlibc/UPSTREAM` пінує тег і коміт, `xtask`
  завантажує дерево в `target/hxlibc/upstream`, накладає `hxlibc/patches/*.patch` і
  копіює `hxlibc/sysdeps/hamix`. Diff із апстрімом -- це саме `patches/` і `sysdeps/hamix`.
* `abis/hamix` поки не потрібен: `sysdeps/hamix/include/abi-bits` посилаються на
  `abis/linux` (розкладка структур Linux, ADR-0006B).
* Прогалини mlibc, помічені на busybox (для драбини портів): `cfsetspeed`, `acct`,
  `struct sysinfo` у `<sys/sysinfo.h>`, BSD-імена в `<netinet/*>` (`ATF_*`, `ICMP_*`),
  `syslog.h` `CODE`/`prioritynames`, `struct seminfo/msginfo`; попередження mlibc про
  `io_seek()` на консолі (тип потоку кешується до перенаправлення).

## Щаблі 1-2 драбини портів (2026-09-28)

* bash 5.2 на hxlibc став системною оболонкою (ADR-0012B).
* Зібрані й перевірені в QEMU: libffi, expat, pcre2, pixman, wayland, libxkbcommon, glib,
  dbus (нативний `dbus-daemon` обслуговує й `hx-login1` з Linux-шару), fribidi,
  fontconfig, cairo, harfbuzz, pango. Бібліотеки -- спільні `.so`, їх вантажить `ld.hx`.
* C++: LLVM libc++/libc++abi/libunwind 22 зібрано під hxlibc (порт `libcxx`, статичні
  архіви); `hx-c++` підключає їх автоматично. Для цього libc++ отримує патч:
  `__hamix__` користується тим самим `support/linux.h`, що й Linux, і власною таблицею
  ctype (`_LIBCPP_PROVIDES_DEFAULT_RUNE_TABLE`).
* Помилки mlibc, знайдені портами (патчі `hxlibc/patches/0004-0008` і `sysdeps/hamix`):
  знаковий `UINT_MAX`; `gettext` без `format_arg`; `getresuid/getresgid` передавали в
  ядро адресу локального вказівника (є і в `sysdeps/linux` апстріму); немає
  `sys_setgroups`, `strxfrm_l`, `wcscoll_l`, `wcsxfrm_l`, `strftime_l`, `catopen`;
  `memccpy` -- "not implemented"; `_POSIX_TIMERS` не оголошено; `C.UTF-8` не
  приймався як C-локаль. Варто віддати апстріму.
* `hx-cc` визначає `__unix__`: clang не визначає його для невідомої ОС `hamix`, а від
  нього залежать libc++ (потоки, `catopen`) та інші порти.
