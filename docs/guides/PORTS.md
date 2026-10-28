# Порти: стороннє ПЗ на hxlibc

Сторонні програми й бібліотеки (bash, busybox, glib, cairo, GTK 3, ...) збираються проти
hxlibc (режим 2 плану B, §4A.7) і лежать окремо від власного коду HamixOS, у дереві
`ports/`, як ports у FreeBSD: `ports/core/bash`, `ports/services/dbus`, `ports/libs/gtk3`.
`ports/` і `userland/` -- сусіди з однаковими ролями (`apps`, `core`, `desktop`, `services`,
`libs`), тож бібліотеки Unix і бібліотеки HamixOS однаково лежать на рівні
`<дерево>/libs/<назва>`, але не змішуються. Каталог порту містить `HXBUILD` (shell-фрагмент
у дусі `APKBUILD` з Alpine): `pkgname`, `pkgver`, `source`, `sha256`, `builddir`, `depends` і
функції `build()` та `package()`, плюс необов'язкові `patches/*.patch`. `cargo xtask apps`
показує і крейти, і порти з їхніми каталогами.

Куди класти новий порт: `ports/apps` -- програми з вікнами, `ports/core` -- команди,
`ports/services` -- демони, `ports/libs` -- бібліотеки й дані для них. Сторонні Rust-крейти,
що живуть у дереві (smoltcp, rust_h264, shiguredo_mp4), -- у `vendor/`. Cargo-робочий
простір складається лише з `userland/*/*` і `vendor/*`, тож `ports/` cargo не бачить.

`cargo xtask ports [NAME...]` fetches and verifies the source, builds it with
`hxlibc/hx-cc`, stages the result in `out/hx-ports/ARCH/root` (the image picks it up)
and writes `docs/reference/hxlibc-status.md`.

Variables available to the recipe: `CC`, `CXX`, `AR`, `RANLIB`, `STRIP`, `CFLAGS`,
`LDFLAGS`, `PKG_CONFIG_*`, `JOBS`, `pkgdir`, `srcdir`, `HX_ROOT` (the staging root with
already built dependencies).

Meson-проєкти: `meson setup build $HX_MESON ...` -- `HX_MESON` містить крос-файл
(`target/hx-ports/ARCH/meson-cross.txt`, `hx-cc`/`hx-c++`, `system = 'linux'`, бо hxlibc
дає Linux-сумісний API), префікс `/usr` і `--wrap-mode=nodownload`. Перед `meson setup`
рецепт робить `unset PKG_CONFIG_LIBDIR PKG_CONFIG_SYSROOT_DIR`: крос-файл уже задає
sysroot, а змінні оточення зламали б пошук інструментів для машини збирання
(приклад -- нативний `wayland-scanner` у `wayland/HXBUILD`).

Бібліотеки збираються спільними (`*.so`, завантажує `/lib/ld.hx`); в образ не потрапляють
`*.a`, `*.la`, заголовки, pkg-config і утиліти для розробки.

Тести на C, що використовують порти: `tests/hxlibc/NAME.c` + `NAME.pkg` зі списком
модулів pkg-config (наприклад `glib-2.0`); їх запускає `tests/boot/native-ports-ladder2.sh`,
GTK 3 (`gtk-hello`) -- `tests/boot/native-gtk.sh` під Nook і hxwayland.

`hx-cc` бачить `-shared`/`-c` і всередині файлів відповідей `@file` (так Meson лінкує
великі бібліотеки на кшталт `libgtk-3.so`).

Додаткові поля рецепта:
* `source`/`sha256` можуть містити кілька значень через пробіл: перший архів -- сам
  пакунок, решта розпаковуються поруч у `$srcdir` (так `fontconfig` збирає для хоста
  потрібний йому `gperf`);
* `extract="шлях ..."` -- розпакувати з першого архіву лише ці шляхи (для монорепо на
  кшталт `llvm-project`).

`cargo xtask ports NAME` перезбирає NAME, а його залежності -- лише коли змінився їхній
`HXBUILD`/патчі (хеш у `out/hx-ports/ARCH/built`) або перезібралась їхня залежність.
`cargo xtask ports` без імен перезбирає все.

C++: порт `libcxx` (LLVM libc++, libc++abi, libunwind, статично) ставить
`usr/include/c++/v1` і `libc++.a` у корінь портів; `hx-c++` бере їх звідти автоматично
(`HX_NO_CXX_RUNTIME=1` вимикає це -- так збирається сам libcxx).

Код, що перевіряє `__linux__` (заголовки DRM, Mesa, libglvnd), отримує його лише там,
де треба: `meson setup build $HX_MESON -Dc_args="-D__linux__ -D_GNU_SOURCE -I$HX_ROOT/usr/include"`
(`c_args` із командного рядка заміняє крос-файл, тому `-D_GNU_SOURCE` і `-I` -- теж).

Після кожної збірки xtask записує список файлів пакунка в
`out/hx-ports/ARCH/files/NAME.list` і видаляє з кореня портів те, що пакунок ставив
раніше, а тепер -- ні.

Ще кілька правил:
* `source="ім'я::url"` (як в Alpine) -- ім'я файла в кеші, коли URL закінчується
  безликим `1.0.tar.gz` (архіви codeberg/github).
* xtask видаляє `*.la` з кожного пакунка: libtool інакше підхоплює `/usr/lib` хоста.
* Файли `.pc`, що повертають шляхи через `--variable` (як `xcb-proto`), мають
  використовувати `${pc_sysrootdir}`, інакше збірка побачить каталоги хоста.
* Код, що перевіряє `__linux__` у встановлених заголовках, краще виправити патчем
  (`defined(__linux__) || defined(__hamix__)`, як у `libdrm`), ніж `-D__linux__` у кожного споживача.
