# ADR-0008B: hxserver і hxproto v2 замість переходу на Wayland

Status: accepted · Date: 2026-09-27 · Plan: §7

## Контекст
Nook (hxserver) говорить власним протоколом hxproto. План A пропонував переписати
Nook як Wayland-композитор; план B лишає hxproto головним, а Wayland -- лише для
Linux-програм (через hxwayland; з ADR-0014B він нативний і обслуговує й GTK на hxlibc).

## Рішення
* hxproto розвивається до v2 із версіонуванням у рукостисканні (`Hello{version}`);
  клієнти v1 працюють без змін.
* Нові ролі й події v2: **layer surface** (anchor, exclusive zone, шар
  background/bottom/top/overlay), **foreign toplevel** (список вікон для панелі/доку),
  **workspaces**, **clipboard з MIME-типами**, **drag-and-drop**, **output з масштабом**,
  **damage**, idle/lock, presentation feedback.
* hxwayland мапить Wayland-об'єкти 1:1 на v2 (layer-shell -> layer surface,
  foreign-toplevel -> foreign toplevel), тому XFCE-панель може стати layer surface.
* Вигляд і анімації Nook не змінюються; еталон -- `tests/nook-golden/` (знімки
  станів + таблиця параметрів з коду), розбіжність = червоний тест.

## Наслідки
* Власний протокол лишається частиною ідентичності; підтримку Wayland-протоколів несе лише hxwayland.
* `hx-toolkit` і `libhxclient.h` відкривають hxproto стороннім програмам.

## Стан hxwayland (2026-09-28)
Глобали: `wl_compositor` 5, `wl_shm` 1, `wl_seat` 7, `wl_output` 4, `xdg_wm_base` 5,
`wl_subcompositor` 1, `wl_data_device_manager` 3 (буфер обміну мостом на
hxproto selection), `zxdg_decoration_manager_v1` 1,
`org_kde_kwin_server_decoration_manager` 1, `zwlr_layer_shell_v1` 4,
`zwlr_foreign_toplevel_manager_v1` 3, `wp_viewporter` 1 (source/destination,
масштабування найближчим сусідом при копіюванні в вікно hxserver),
`wp_fractional_scale_manager_v1` 1 (зараз завжди 120 = 1.0),
`zxdg_output_manager_v1` 3.

Відправка клієнтам неблокуюча: у кожного клієнта своя черга (дескриптори
дублюються, поки повідомлення в черзі), доочищується за `POLLOUT`; клієнт, що
не читає й набрав понад 8 МіБ, відключається замість того, щоб гальмувати
весь міст.

`zwp_linux_dmabuf_v1` свідомо не оголошено: поки `hx-drm-compat` не має
PRIME/`renderD128`, клієнтам нема чого імпортувати, а оголошений, але
порожній протокол ламає шлях Mesa через `wl_shm`. Перевірка:
`wl-layer-check --toplevels --clipboard --protocols` у
`tests/boot/linux-layer-shell.hsh`.
