# ADR-0017B: дерево `ports/` окремо від `userland/`, C-бібліотеки HamixOS

Status: accepted · Date: 2026-10-05 · Plan: UNIX_PLAN U5 · Уточнює: ADR-0015B

## Контекст
* ADR-0015B розклав програми за роллю, але в одному каталозі опинилися і власні крейти
  HamixOS, і рецепти стороннього ПЗ: `userland/libs` містив `hxstd`, `hxproto`, `hxtoolkit`
  поруч із `gtk3`, `glib`, `mesa`, `libX11` (71 каталог). Що написано для HamixOS, а що --
  порт, видно було лише всередині каталогу (`Cargo.toml` чи `HXBUILD`).
* Сторонні Rust-крейти (`smoltcp`, `rust_h264`, `shiguredo_mp4`) лежали серед власних.
* Бібліотеки HamixOS існували лише як Rust-крейти: C/C++-програма не могла намалювати
  вікно на hxserver, бо не було ні `.so`, ні заголовків, ні `pkg-config`.
* Клієнтський транспорт hxserver (сокет, `shm_open`, вікно) жив усередині hxtoolkit.

## Рішення
* **Два дерева з однаковими ролями**, як `src` і `ports` у FreeBSD:
  * `userland/<роль>/<назва>` -- код HamixOS (крейти);
  * `ports/<роль>/<назва>` -- стороннє ПЗ з рецептом `HXBUILD` і патчами;
  * ролі однакові: `apps`, `core`, `desktop`, `services`, `libs`;
  * `vendor/` -- сторонні Rust-крейти, що живуть у дереві.
  Робочий простір Cargo: `userland/*/*` і `vendor/*`; xtask шукає рецепти лише в
  `ports/*/*/HXBUILD`, `exclude` у кореневому `Cargo.toml` знову короткий.
* **C-бібліотеки:** крейт із `crate-type = ["cdylib"]` і `clib = "назва"` (+ `soversion`) у
  `[package.metadata.hamix]`. xtask збирає його під hamix-ціль і ставить:
  * у образ -- `/usr/lib/lib<назва>.so.<soversion>` (SONAME з `build.rs`);
  * у корінь розробки `out/hx-ports/ARCH/root` -- `.so`-посилання, `include/` крейта і
    `<назва>.pc` з `<назва>.pc.in` (`@VERSION@` = версія крейта).
  Порти й C-тести знаходять бібліотеку через `pkg-config`, як будь-яку бібліотеку Linux.
* **`hxproto::client`** (фіча `client`, `no_std + alloc`, лише функції libc): з'єднання з
  hxserver, вікно, спливні поверхні, спільна пам'ять. hxtoolkit (бекенд Nook) і
  `libhxproto` користуються одним кодом.
* **`libhxproto.so.0`** (`userland/libs/libhxproto`): C API поверх `hxproto::client` --
  `hx_window_create`, `hx_window_pixels`, `hx_window_present[_rect]`, `hx_window_resize`,
  `hx_window_next_event` (+ `hx_window_fd` для `poll`), заголовок/app-id/іконка/курсор,
  сповіщення, буфер обміну. Бібліотека `no_std`, власний алокатор поверх `malloc`, у
  `NEEDED` лише `libhxc.so`.

## Наслідки
* Новий порт -- `ports/<роль>/<назва>/HXBUILD`; нова бібліотека HamixOS --
  `userland/libs/<назва>`; якщо вона потрібна C-коду -- поруч крейт `lib<назва>` з `clib`.
* Хеш рецепта не залежить від шляху, тож переїзд не перезбирає порти.
* Одне вікно на процес: адреса клієнта -- `/run/hamix/ipc/<pid>`. Кілька вікон (як
  «Properties» у Files) -- кілька процесів, доки hxproto не отримає мультиплексування.
