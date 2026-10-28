# ADR-0014B: GTK 3 на hxlibc і нативний hxwayland

Status: accepted · Date: 2026-09-29 · Plan: §4A.7 (щабель 3), §7 · Уточнює: ADR-0007B, ADR-0008B

## Контекст
* Щабель 3 драбини портів -- `gtk3` на hxlibc. GTK 3 має бекенди X11, Wayland і
  Broadway; нативного бекенду hxproto немає, і писати його -- це підтримувати форк GTK.
* hxwayland (Wayland-композитор поверх hxproto) лежав у `layers/linux/`, хоча це
  звичайна нативна програма на `hxstd`: у Linux-шарі була лише його роль "міст для
  Linux-програм". У профілі `hamix-core` hxwayland не було, тож нативний GTK там не
  мав би дисплея.

## Рішення
* **Нативний GTK 3 говорить Wayland** (`-Dwayland_backend=true`, без X11 і Broadway),
  hxwayland -- його дисплей. Протокол HamixOS лишається hxproto; Wayland -- сумісний
  шар для тулкітів, як і планувалося в ADR-0008B.
* **hxwayland і `wl-layer-check` переїхали в `userland/desktop/`** і є в обох профілях.
  Xwayland лишається частиною Linux-шару: hxwayland запускає `/linux/usr/bin/Xwayland`,
  лише якщо він є. `lint-layers` більше не вважає згадку hxwayland у нативних
  маніфестах порушенням.
* **Порти GTK 3:** `wayland-protocols`, `khronos-headers` (EGL/KHR, `EGL_NO_X11`),
  `libepoxy` (лише EGL), `gdk-pixbuf` (вбудований PNG), `libxml2`, `atk`
  (`at-spi2-core -Datk_only=true`), `xkeyboard-config`, `gtk3`; `cairo` -- з
  `cairo-gobject`. Усі -- спільні бібліотеки в `/usr/lib`.
* **Тема Nook** (`/usr/share/themes/Nook`) -- нативна; `/linux/usr/share/themes/Nook` --
  симлінк на `/hamix/...`, тож Linux-GTK бачить ту саму тему.
* **hxlibc:** `gettext`-родина -- тотожний каталог (повертає `msgid`, `textdomain` і
  `bind_textdomain_codeset` пам'ятають значення) замість `__ensure` (патч 0010); `hx-cc`
  читає `-shared`/`-c` із `@file`.

## Наслідки
* `tests/boot/native-gtk.sh` (обидва профілі): `gtk_init_check` без композитора ->
  `FALSE`; під Nook -- `GdkWaylandDisplay`, тема Nook, вікно намальоване, сигнал кнопки.
* Переклади GTK не працюють (англійський інтерфейс) -- до справжнього `libintl`.
* Далі за драбиною: `mesa` (swrast) для `libepoxy`/GL, `gtk4`, `qt6`-базові модулі;
  нативні GTK-програми можуть з'являтися в меню Nook через `.desktop`.
