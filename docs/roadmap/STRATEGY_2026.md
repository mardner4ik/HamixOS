# HamixOS: стратегія до кінця 2026 ("Один світ")

Дата складання: 2026-09-24. Дедлайн: 2026-12-31 (~14 тижнів, тижні W1..W14).
Стартова точка: HamixOS 0.6.1, Nook на власному протоколі `hxproto` (hxserver),
`hxwayland` як міст для Linux-клієнтів (+ XWM для X11), Linux ABI майже повний
(Telegram працює, Chromium ще падає), 3 архітектури, простій ~70 МБ з Nook.

## 0. Суть зміни напрямку

Було: два світи. Нативні програми говорять з Nook через `hxproto`, Linux-програми
-- через `hxwayland`-міст, кожен світ має свій ABI (`SYS_HAMIX_*` vs Linux),
свої вікна, свої іконки, свій life-cycle.

Стає: один світ.

1. **Один протокол вікон -- Wayland.** Nook = Wayland-композитор. Нативні
   програми -- звичайні Wayland-клієнти (на `hx-toolkit`), а не гості hxproto.
2. **Один ABI для користувацького простору -- Linux-сумісний** (musl, ті самі
   syscall-и, /proc, /sys, netlink, epoll, futex). `SYS_HAMIX_*` майже
   зникають (з 48 лишається 0-5, §6.2), жодна програма більше не є "чужою". Слово "linuxulator" зникає з UI і документації.
3. **Один композитор, дві оболонки:** Nook і XFCE -- це просто різні набори
   Wayland-клієнтів (панель, док, робочий стіл, лаунчер) поверх того самого
   композитора з `wlr-layer-shell`. Перемикання DE = зміна сесії в екрані входу.
4. **Пам'ять як фіча:** бюджет прописаний і міряється в CI.

> УВАГА: це прямо змінює керівний принцип з `docs/roadmap/ARCHITECTURE_ROADMAP.md`
> ("нативний шлях спершу, Linux ABI лише для чужого софту"). Новий принцип:
> **"немає чужого софту"**. Нативний ABI стає шаром розширень над Linux-ABI,
> а не паралельним світом. Після W1 треба оновити ARCHITECTURE_ROADMAP.md,
> README ("Two ABIs") і правило "спершу SYS_HAMIX_*".

## 0.1 Ідентичність: чим HamixOS лишається собою [MUST, ADR-0010]

Побоювання: чим більше Linux-сумісності, тим більше проєкт схожий на "ще один
Linux". Відповідь: **Linux-сумісність -- це інтерфейс, а не реалізація.** Так
працюють WSL1, Linuxulator у FreeBSD, gVisor: під капотом інше ядро, зверху --
знайомий ABI і софт. HamixOS -- це власне ядро на Rust плюс власна стратегія
пам'яті, а не дистрибутив Linux.

Що є тільки в HamixOS і що ми **не віддаємо** (це і є продукт):
1. **Власне ядро на Rust** (без коду Linux): пам'ять безпечна за побудовою,
   ядро ~десятки тисяч рядків, яке одна людина може тримати в голові.
2. **Жорсткий бюджет пам'яті як гарантія:** Nook <=70 МБ, XFCE <=200 МБ, міряється
   в CI (`membench`), регресія = червоний CI. У жодного звичайного дистрибутива
   такої гарантії немає. Модулі ядра мають ліміт (`BUDGET_BYTES`), ленива
   пам'ять (`hamix-lazy-memory`), драйвери завантажуються лише за наявності заліза.
3. **Швидкий старт і невеликий образ:** ціль <=6 с до робочого столу в QEMU,
   образ без зайвого (ніякого systemd, ніяких важких демонів за замовчуванням).
4. **Nook** -- власна DE з характерними анімаціями (лампа, тощо), збережена 1:1.
5. **Модульні драйвери через KPI** -- ядро не роздувається під чуже залізо.
6. **`hext` і атомарні оновлення з відкатом** (знімки для pantry) -- те, чого
   Linux-дистрибутиви не мають "з коробки" в такому легкому вигляді. [SHOULD]
7. **Rust-first системні компоненти:** `hxcomp`, `hx-toolkit`, `hxfiles`... на
   безпечній мові.

Що ми **свідомо не робимо**, щоб не стати "легким Linux":
* не беремо код ядра Linux, не портуємо DRM-ядро/cgroups/namespaces/BPF/systemd;
* не вимагаємо повної сумісності з усім: якщо програма потребує рідкісної
  можливості Linux і вона коштує пам'яті чи коду -- не підтримуємо;
* не женемося за апаратним прискоренням на реальному залізі: у 2026 -- лише
  ті сходинки, що вже описані (§6.5), решта -- не блокує реліз.

Правило "не гірше за Tiny Core у розумному сенсі": ціль -- **не найменший образ,
а найкраща пропорція "сумісність / пам'ять / простота ядра"**: повноцінний
десктоп зі справжнім Linux-софтом за ціною пам'яті, значно нижчою за типовий
дистрибутив (Alpine+XFCE зазвичай 300+ МБ).

**Заморожування стратегії.** Напрямок (власне ядро + Linux-ABI + Wayland + Alpine
apk + OpenRC) фіксується на весь 2026 рік ADR-ами 0005-0010. Зміна будь-якого з
них -- лише через новий ADR з обґрунтуванням і оцінкою вартості, а не на ходу.
Причина: за останні місяці стратегія міняла напрямок двічі (linuxulator, потім
відмова від нативних драйверів на користь KPI), і це коштувало робіт, які
довелося переробляти.

## 1. Чесні застереження (прочитати перед усім)

* **XFCE і Wayland (рішення власника проєкту).** Розраховуємо, що XFCE до кінця
  року отримає Wayland-підтримку (4.22), тому XFCE у нас **одразу Wayland-нативний**:
  жодного Xwayland-сеансу як мети. Композитор `hxcomp` реалізує протоколи, яких
  XFCE-на-Wayland очікує від wlroots-подібного композитора (layer-shell,
  foreign-toplevel, output-management, workspace, session-lock, idle-notify,
  screencopy, xdg-activation -- точний список звіряємо з `libxfce4windowing` і
  `xfce4-panel` у момент початку роботи). До релізу 4.22 працюємо на dev-релізах
  (4.21.x) і збираємо їх у `pantry`. **Ризик:** якщо реліз зсунеться, ми залежимо
  від чужого графіка. Страховка: Xwayland (rootless) лишається лише для окремих
  X11-програм, а X11-сеанс XFCE -- як аварійний варіант, а не як віха.
* **200 МБ з XFCE -- досяжно, але не "з коробки".** Типовий XFCE на Linux займає
  350-500 МБ. Ціль 200 МБ значить: без gvfs/tumbler/polkit-agent/colord/
  pulseaudio-демонів за замовчуванням, `xfce4-session` з ледачим стартом,
  спільні бібліотеки один раз у page-cache, стиснена swap (zram), налаштований
  malloc. Метрика має бути чітко визначена (див. §4), інакше цифри порівнювати
  нема з чим.
* **Порівняння з Redox.** Redox сильніший у мікроядерній архітектурі, чистоті
  Rust-портів і самохості. Наш козир -- **запускає готовий Linux-софт і Wayland**
  при малому споживанні. Конкурувати треба цим, а не "теж мікроядро".
  Друга половина конкуренції -- **професійність проєкту** (порядок у репозиторії,
  системі, документації, релізах): див. §5.0.
* **14 тижнів -- це мало для всього.** Нижче кожен пункт помічений
  `[MUST]` / `[SHOULD]` / `[COULD]`. Якщо щось зривається, різати треба з кінця
  списку COULD, а не розмазувати якість. Ризик №1: Chromium/Firefox і GTK4 --
  невідомий обсяг помилок ядра/syscall.

## 2. Цільова архітектура

```
 apps (Rust native, GTK/Qt, Electron, XFCE apps)  -- всі Wayland-клієнти або X11 через Xwayland
        |  wayland (+ xdg-shell, layer-shell, xdg-decoration, viewporter,
        |            wp-fractional-scale, linux-dmabuf, wl_shm, data-device,
        |            text-input-v3, idle-inhibit, foreign-toplevel, session-lock)
 hxcomp  (композитор: еволюція hxwayland + window-менеджмент з hxserver/wm.rs)
        |  DRM/KMS-подібний шлях: virtio-gpu / intel-display / fb, aarch64, riscv64
 shells: nook-panel/dock/launcher/desktop   |   xfce4-panel/xfdesktop/appfinder (Wayland-нативні)
 services: OpenRC (PID 1), dbus (легкий), pipewire-lite/hxsound, NetworkManager-lite = hxnet
        |
 ядро: Linux-ABI first + SYS_HAMIX_* розширення, hext, модулі (.ko-подібні)
```

Рішення про композитор: **не переходити на wlroots/smithay**, а розвивати
`hxwayland` (1730 рядків main + 1074 xwm) у `hxcomp`. Причини: пам'ять,
відсутність залежності від libdrm/gbm/EGL-стеку Linux, вже працює. Ціна --
самим писати layer-shell, dmabuf, fractional-scale. Це закладено в план.

## 3. Пам'ять: бюджет (ціль)

| Компонент                                   | Nook-профіль | XFCE-профіль |
|---------------------------------------------|--------------|--------------|
| Ядро + hext + модулі                        | 22 МБ        | 22 МБ        |
| hxinit + сервіси (net, sound, dbus-lite)    | 8 МБ         | 12 МБ        |
| hxcomp (композитор + Xwayland лише за потреби) | 14 МБ     | 20 МБ        |
| Shell (Nook: panel/dock/launcher)           | 12 МБ        | 0            |
| XFCE (panel, desktop, session, settingsd)   | 0            | 70 МБ        |
| Термінал + файловий менеджер (відкриті)     | 10 МБ        | 30 МБ (Thunar+xfce4-terminal) |
| Буфери фреймів / кеші                       | 4 МБ         | 30 МБ        |
| Разом                                       | **<=70**     | **<=200**    |

Прийоми (кожен -- окрема задача в плані):
* **Ледачий старт** усього, що не потрібно на першому кадрі (D-Bus activation +
  `hx-activate`: sound, xdg-portal, Xwayland; OpenRC -- для решти, §6.4).
* **Спільні сторінки для бібліотек:** file-backed mmap з page-cache без копіювання
  (доробити lazy PT_LOAD -- це також лагодить Chromium, див. `hamix-linux-app-blockers`).
* **zram + KSM-lite** для анонімних сторінок GTK-процесів.
* **musl mallocng -> налаштований** (`MALLOC_*`-еквіваленти, повернення пам'яті ядру
  через `madvise(MADV_FREE/DONTNEED)`), для нативних Rust-програм -- `hx-alloc` з
  малими arena.
* **Один процес на "родину" GTK-апок** не робимо; замість цього мінімізуємо
  `libgtk` залежності (без cups, без a11y-bus, без gvfs через `GIO_USE_VFS=local`).
* Вікна: SHM-буфери з **damage-tracking і повторним використанням**, ніяких
  копій на кожен кадр; software-рендер з тайлами.
* Шрифти: один спільний набір (без дублювання Noto), mmap, без завантаження CJK на старті.

## 4. Як міряти (без цього план не має сенсу) [MUST, W1]

Метрика "простій" = `MemTotal - MemAvailable` через 60 с після появи робочого
столу, без відкритих вікон, у QEMU `-m 1G -smp 2`, віртуальна відеокарта 1280x800.
Додатково окремо: RSS/PSS по процесах (`hxmon`), page-cache окремо.
* `tools/membench` -- скрипт QEMU, що вантажить образ, чекає готовності, знімає
  `/proc/meminfo` + `/proc/*/smaps_rollup` (PSS), зберігає JSON.
* Таблиця "бюджет vs факт" у README, оновлюється в CI на кожен merge.
* Швидкість ядра міряє `kbench` (§6.6) за тим самим принципом.
* Другі метрики: час завантаження до робочого столу (ціль <=6 с у QEMU),
  затримка руху миші/вікна (<=1 кадр), розмір ISO.

## 5. Календарний план

### 5.0 Наскрізний напрямок: порядок у проєкті і в системі [MUST, W1-W3, далі постійно]

Діагноз (з поточного стану репозиторію, 2026-09-24):
* Корінь репозиторію змішує код, артефакти й дані: `hamix_disk.img` (256 МБ),
  `hamix_os.iso` (126 МБ), `rootfs/`, `rootfs-aarch64/`, `rootfs-riscv64/`,
  `isoroot/`, `overlay/`, `target/` -- половина з цього збірка, а не джерела.
* `overlay/` (864 файли, 34 МБ) -- це один шматок без власників: невідомо, який
  файл до якого компонента належить. `overlay/opt/linux/{bin,usr,etc,lib}` -- це
  окремий "Linux-світ" усередині образу, тобто саме той розділ, який нова
  стратегія скасовує.
* Застосунки, сервіси, бібліотеки й драйвери лежать у `apps/`, `libs/`, `sdk/`,
  `drivers/` без принципу: `hxinit` -- в ядрі (`kernel/src/hxinit.rs`), пакетник
  `pantry` -- у `apps/`, а "нативні" й "Linux" збірки -- у `tools/linux-sysroot`.
* `build.sh` -- 400 рядків shell, де сплетені збірка, образи, модулі й запуск.
* `Cargo.lock` у `.gitignore`, хоча це бінарний проєкт -- збірки не відтворювані.
* `CHANGES.md` -- 114 КБ в одному файлі; `docs/` -- 20+ документів, частина
  історичних (`USERSPACE_ROADMAP`), частина плани, частина довідники, без індексу
  і без позначки статусу.

Цільова структура репозиторію (одним перенесенням `git mv` у W1-W2, до розходження гілок):

```
HamixOS/
  kernel/            ядро; arch-специфічне тільки в kernel/src/arch/{x86_64,aarch64,riscv64}
  modules/           було drivers/: зовнішні модулі ядра (virtio-gpu, xhci, ahci, ...)
  sdk/               hamix (тонкі SYS_HAMIX_*), hamix_kpi, hamix_linuxkpi (ABI модулів ядра); hamix_std -> видалити (§6.1)
  libs/              спільні бібліотеки userland (hext, hxvt, hx-toolkit, кодеки)
  userland/
    core/            hsh, pantry, hxmon, hed, базові утиліти
    services/        hxnet, hxsound, hx-portal, hx-session, dbus-обгортка
    desktop/         hxcomp (композитор), nook-panel, nook-dock, nook-launcher,
                     nook-desktop, hx-greeter, hxsettings
    apps/            hxfiles, hxterm, hxnotes, hxcalc, hxview, hxvideo, hxsound-ui, hxinstall
  packages/          рецепт на пакунок: <name>/{APKBUILD, files/, patches/} (формат Alpine, ADR-0008)
                     (замінює overlay/ і tools/linux-sysroot; кожен файл в образі
                     має власника-пакунок, XFCE/GTK/Qt лежать тут же)
  data/              брендинг, іконки Nook Icons, шпалери, шаблони .desktop, ліцензії
  tools/             xtask, mkhext, membench, nook-assets, linuxkpi-скрипти
  tests/             qemu-сценарії, wayland-клієнти для протоколів, скриншот-еталони
  docs/              див. нижче
  out/               (gitignored) образи, iso, rootfs, isoroot, логи -- усе згенероване
```

Правила, які закріплюємо документом `CONTRIBUTING.md`:
1. **Усе згенероване -- лише в `out/`.** У корені жодних `.img`/`.iso`/`rootfs*`.
2. **Одна точка входу збірки: `cargo xtask`** замість `build.sh`
   (`build`, `run`, `test`, `iso`, `membench`, `fmt`, `lint`, `release`). Кожна
   підкоманда має `--arch`. Shell лишається тільки там, де без нього не обійтись.
3. **`Cargo.lock` у git**, пінована версія toolchain (вже є), пінований коміт для
   кожного зовнішнього джерела (musl, GTK, XFCE) у `packages/*/manifest.toml`.
4. **Кожен файл в образі належить пакунку** (як у нормальному дистрибутиві):
   `pantry files <path>` показує власника, `pantry verify` ловить сторонні файли.
5. **Іменування:** `hx*` -- нативні системні компоненти, `nook-*` -- частини Nook,
   решта -- імена апстріму. Один префікс на один рівень, без винятків.
6. **Кожна публічна утиліта:** `--help`, `--version`, коректні коди виходу, man-сторінка.
7. **`rustfmt` + `clippy` + збірка трьох архітектур + boot-тест -- обов'язкові в CI**;
   merge у `main` лише зеленим PR.

Документація ("є доступний індекс, у кожного документа є статус"):
```
docs/
  README.md          індекс з посиланнями і статусом кожного документа
  architecture/      ядро, hext, ABI, композитор, пам'ять, мережа, драйвери
  guides/            інсталяція, команди, hsh, pantry, портування софту
  reference/         syscall-таблиця, протоколи Wayland, які підтримуються, .desktop-ключі
  adr/               Architecture Decision Records: 0001-single-abi.md,
                     0002-wayland-native.md, 0003-own-compositor.md, 0004-toolkit.md ...
  roadmap/           цей файл (STRATEGY_2026), квартальні плани
  history/           USERSPACE_ROADMAP та інші застарілі -- лишаються, але не в основі
  licenses/          -> THIRD_PARTY_NOTICES.md (автоматично з packages/)
```
Кожен документ має шапку `Status: current | draft | historical`, `Owner`, `Updated`.
Кожне ключове рішення цього плану оформлюється ADR у W1-W2 -- "чому Wayland,
чому один ABI, чому власний композитор" -- щоб рішення не жили лише в голові.

Релізи й зміни:
* `CHANGES.md` -> `CHANGELOG.md` у форматі Keep a Changelog + окремі
  `docs/history/changes/0.6.x.md`; SemVer (0.7.0 -> 1.0 після стабілізації ABI);
  гілки `main` (розробка) і `release/0.7`; теги підписані.
* Шаблони issue/PR, мітки (`area:kernel`, `area:comp`, `area:xfce`, `blocker`),
  `SECURITY.md`, `CODE_OF_CONDUCT.md`, `THIRD_PARTY_NOTICES.md`.
* Публічна сторінка "що працює" (application compatibility list) з `tests/`,
  яка генерується автоматично.

Порядок у самій системі (файлова система образу, FHS-стиль):
* **Один розклад без `/opt/linux`:** як у пінованому релізі Alpine (§6.4);
  програми Hamix і портовані лежать разом, різниця лише в іменах пакунків.
* `/etc/hamix` -- системні налаштування, `/etc/xdg` -- налаштування DE,
  `/etc/init.d`, `/etc/conf.d`, `/etc/runlevels` -- сервіси OpenRC (§6.4),
  `/var/lib/pantry` -- база пакунків,
  `/var/log/hx` -- журнали, `/run` -- сокети (`wayland-0`, `dbus`).
* **Один журнал:** `hxlog` (кільцевий буфер + файл, `hxlog -f`, рівні), а не
  розкидані `dmesg`, `/tmp/*.log` і stderr. Кожен сервіс пише в нього.
* **Один спосіб налаштувань:** `hxsettings` <-> `gsettings`/`xfconf` <-> файли
  в `/etc/hamix`, без дублювання й без "прихованих" конфігів у коді.
* **Одна довідка:** `man`, `--help` і `hxhelp` виводять одне й те саме.
* Вбудовані шляхи (`/usr/share/nook`, `/usr/share/hamix`, шпалери, іконки) --
  єдиний модуль `hamix_paths` у `sdk`, а не константи в кожній програмі.

Вихід W3: структура перенесена, `cargo xtask build/run/test` працює на трьох
архітектурах, документація має індекс і ADR, `main` захищено CI.

### W1 (24.09-30.09) Фундамент і рішення
* [MUST] Прийняти цей документ; оновити ARCHITECTURE_ROADMAP.md і README (новий принцип).
* [MUST] **Порядок у проєкті (§5.0): перший етап -- зробити ТУТ, до будь-яких
  паралельних гілок**, бо перенесення директорій після розходження гілок дає
  конфлікти. Структура репозиторію, `xtask`, перегляд документації -- W1-W2.
* [MUST] `tools/membench` + перша таблиця бюджету (базова лінія 0.6.1).
* [MUST] **Золотий еталон Nook (§6.3):** скриншоти всіх станів, покадрові
  послідовності анімацій і таблиця параметрів -- ДО початку перепису.
* [MUST] Заморозити `hxproto` як **застарілий**: перелік того, що нативні апки з
  нього використовують (вікно, буфер, події, буфер обміну, dnd, іконки, SetAppId),
  щоб перенести 1:1 на Wayland.
* [MUST] Тестовий стенд: QEMU-сценарії x86_64 (q35, virtio-gpu) як регресійний
  набір: boot, desktop, foot, Telegram, GTK3-app. Запуск одним `cargo xtask test`.
* [SHOULD] Вибрати мову/тулкіт для нативних апок: див. §6.

### W2-W3 (01.10-14.10) Wayland-протокол до рівня "будь-який клієнт мапиться"
Це найбільший блокер (Qt/Chromium не мапить вікно, див. `hamix-linux-app-blockers`).
* [MUST] Повний `xdg-shell` (popups, positioner, grab, resize/move requests,
  min/max, fullscreen, maximized, configure/ack серіалізація).
* [MUST] `wl_seat` повністю: pointer (axis, frame, relative), keyboard (xkb keymap
  через shm, repeat_info, modifiers), touch мінімум.
* [MUST] `wl_data_device` (clipboard + DnD), `primary-selection`.
* [MUST] `wp-viewporter`, `wp-fractional-scale-v1`, `xdg-decoration`, `xdg-output`,
  `presentation-time`, `wl_subcompositor` (Chromium/Qt їх вимагають).
* [MUST] Не блокуватися на повільному клієнті: неблокуюча відправка з чергою
  замість `send()`+sleep 1 мс (зараз клієнт, що не читає, вішає весь десктоп).
* [MUST] `zwp_linux_dmabuf_v1` (або чітка відмова) + `wl_drm`-заглушка, щоб Mesa
  llvmpipe/EGL не падали.
* Тести: `weston-info`, `wayland-info`, `weston-simple-shm`, `gtk3-demo`,
  `qt6 hello`, `foot`, `mpv --vo=wlshm`, Telegram.
* Вихід W3: Qt6 і GTK4 "hello" мапляться і реагують на ввід.

### W3-W5 (07.10-28.10) Один ABI: закриваємо прогалини ядра
* [MUST] Lazy `PT_LOAD` для `execve` (шлях `map_lazy`) -- потрібно і Chromium
  (238 МБ), і економії пам'яті (тільки торкнуті сторінки резидентні).
* [MUST] Спільні file-backed mmap через page-cache (MAP_SHARED і MAP_PRIVATE CoW),
  `memfd_create`, `/dev/shm`, `mremap`, `madvise`, `mincore` -- коректні.
* [MUST] `inotify`, `eventfd`, `timerfd`, `signalfd`, `pidfd`, `epoll` edge cases,
  `fcntl` locks, `flock`, `sendmsg SCM_RIGHTS` для Wayland fd-passing.
* [MUST] `/proc` і `/sys` мінімум, що читають GTK/GLib/Qt/Chromium/systemd-free
  дистрибутиви (cpuinfo, meminfo, self/maps/smaps, cgroup-stubs, dmi, drm, backlight).
* [SHOULD] Chromium #GP (fixed offset 0x4550558) -- дебаг з `syscalltrace`;
  часто це `arch_prctl`/TLS/`rseq`/`getrandom`/`clone3` поведінка. Виділити 3 дні.
* [SHOULD] Потоки: `clone3`, `set_robust_list`, `rseq`, `futex_waitv`.
* [MUST] **Об'єднання ABI: відмова від `sdk/hamix_std` (ADR-0005, §6.1) і від
  `SYS_HAMIX_*` (ADR-0006, §6.2, з 48 лишити 0-5):**
  вся нативна userland збирається стандартним `std` під `*-unknown-linux-musl`.
  Крейт `hamix` -- тільки для того, що не має Linux-еквівалента (0-5 викликів). Один шлях fd-таблиці, один шлях процесів.
* Вихід W5: Chromium/Firefox (хоча б один) показує сторінку; Electron-hello працює.

### W4-W7 (14.10-11.11) Nook -> Wayland-нативний shell
Ключова архітектурна робота.
* [MUST] `hxserver` розщеплюється: (a) композитор+WM -> `hxcomp` (з hxwayland +
  `wm.rs`, `anim.rs`), (b) shell-клієнти -> окремі Wayland-процеси:
  `nook-panel`, `nook-dock`, `nook-launcher`, `nook-desktop` (шпалери, іконки).
  Вони використовують `wlr-layer-shell-unstable-v1`, `xdg-popup`, `foreign-toplevel`.
* [MUST] `hx-toolkit`: тонка Rust-бібліотека над Wayland (`wl_shm`, `xdg-shell`,
  шрифти, віджети). Основа для нативних апок. **Мета -- <=1.5 МБ RSS на "hello"
  вікно.** Замінює `hxclient`/`hxproto` клієнт.
* [MUST] Міграція нативних апок на `hx-toolkit` у порядку: `hxterm` (вже майже
  foot-подібний), `hxfiles`, `hxsettings`, `hxnotes`, `hxcalc`, `hxview`,
  `hxvideo`, `hxsound`, `hxmon`, `hxinstall`, `hed` (TUI лишається TUI).
* [MUST] Ті самі `.desktop`-файли й `X-Nook-*` ключі (реєстр apps.rs залишається),
  іконки через `SetAppId` -> просто `app_id` + foreign-toplevel (двигун зі
  `identify_window` переїжджає в панель).
* [MUST] **Вигляд і анімації Nook зберігаються (§6.3):** `anim.rs`, `wm.rs`,
  `render.rs` переносяться в `hxcomp` без зміни логіки; CI порівнює з еталоном.
  Профіль "ecomode" лише додає можливість вимкнення на слабкому залізі.
* [SHOULD] Сесія: OpenRC -> `hx-session` -> композитор -> shell, як окремі
  супервізовані одиниці (перезапуск панелі без втрати вікон -- гарна демка).
* [MUST] Зворотна сумісність на перехід: до кінця W7 `hxproto`-режим доступний
  прапором; після W8 видаляється.
* Вихід W7: весь Nook і всі нативні апки -- Wayland-клієнти; `hxproto` не
  потрібен; регресія пам'яті <= +5 МБ від бази.

### W6-W12 (28.10-23.12) XFCE як Wayland-нативна DE
Ідея: XFCE не "запускається в Nook", а є **другою повноцінною сесією** над тим
самим `hxcomp`. Тому головне -- не портування самих програм (вони звичайні GTK),
а якість Wayland-протоколів композитора й легкий стек навколо.
* W6-W8 [MUST] Пакування в `pantry` (musl): dev-збірки XFCE 4.21.x -- xfce4-session,
  xfce4-panel, xfdesktop, xfce4-settings, xfconf, libxfce4util/ui/windowing,
  garcon, exo, Thunar, xfce4-terminal, xfce4-appfinder, mousepad (є), ristretto.
  Залежності: GTK3 (і GTK4, якщо 4.22 цього вимагатиме), gtk-layer-shell, dbus,
  polkit-мінімум без агента. Кожен пакунок збирається з пінованого коміту.
* W6-W9 [MUST] Протоколи `hxcomp` під вимоги XFCE (звірити з `libxfce4windowing`):
  layer-shell, foreign-toplevel(+ext-foreign-toplevel-list), output-management,
  workspace, session-lock, idle-notify, xdg-activation, screencopy. Кожен --
  з тестовим клієнтом у `tests/wayland/`.
* W8-W10 [MUST] Екран входу `hx-greeter` (Wayland-клієнт) із вибором сесії (Nook /
  XFCE) за `/usr/share/wayland-sessions/*.desktop`; `hx-session` запускає composer +
  shell + dbus session-bus + xfconfd та стежить за ними.
* W9-W11 [MUST] Проходження "реального дня" в XFCE: панель, меню, трей, апплети,
  перемикання вікон/просторів, буфер обміну, drag-and-drop, блокування екрана,
  звук, мережа, скріншот, налаштування. Кожен збій -> тест у `tests/`.
* W9-W12 [MUST] Профілювання пам'яті XFCE: ціль W9 <=260 МБ, W12 <=200 МБ (§3).
  Без Xwayland на старті -- запускається ледачо, лише коли відкривають X11-програму.
* W10-W12 [SHOULD] Перехід із dev 4.21.x на реліз 4.22, щойно він вийде; якщо ні --
  фіксуємо останній dev-снепшот як "XFCE (preview)" у 0.7 і не блокуємо реліз.
* [SHOULD] Тема "Nook" для GTK3/GTK4 і XFCE (іконки Nook Icons вже є); одна
  спільна схема налаштувань (gsettings/xfconf), `hxsettings` пише в неї для обох DE.
* [COULD] Аварійний X11-сеанс XFCE (xfwm4 у rootful Xwayland), якщо Wayland-шлях
  не встигає стабілізуватися; не рекламується.
* Вихід W12: XFCE-Wayland-сеанс стабільно 1 годину, `membench` <=200 МБ (або
  чесна фактична цифра).

### W2-W12 (01.10-23.12) GPU: DRM-шар, Mesa, прискорення (§6.5)
* W2-W3 [MUST] ADR-0009 і дизайн KPI v7 (група "drm"); мінімальний
  `/dev/dri/card0` + sysfs; dumb-буфери і page-flip на `virtio-gpu`/`intel-display`;
  `hxcomp` переходить із `FBMAP` на DRM/KMS (разом із §6.2).
* W4-W7 [MUST] GEM/PRIME/dma-buf, `syncobj`; `libdrm`, `gbm`, Mesa `swrast`
  (Alpine); `wl_drm`/`linux-dmabuf` реально працюють; `kmscube` показує кадр.
* W6-W10 [MUST] `virtio-gpu` 3D (`virgl`): capsets, `RESOURCE_CREATE_3D`,
  `SUBMIT_3D`, blob/host-visible пам'ять, fences. `glmark2-es2-wayland` працює
  в QEMU з GL.
* W8-W12 [SHOULD] Опційний GLES2-бекенд `hxcomp` (профіль "GPU"), direct scanout,
  оверлейні площини для `hxvideo`, GTK4/Firefox на GL.
* W10-W12 [SHOULD] Розвідка Intel (`iris`): ціль-покоління, обсяг, ADR; Venus --
  [COULD].
* Вихід W12: GL-клієнти прискорені у VM (virgl), Mesa не вантажиться в базовому
  профілі, бюджет "GPU" <= +25 МБ, вигляд збігається з еталоном Nook.

### W1-W12 (24.09-23.12) Швидкість ядра (§6.6)
* W1-W2 [MUST] `kbench` + `hxprof` + еталон Alpine у тому ж QEMU; lockdep-lite і
  стрес `-smp 4` у CI; перша таблиця "Hamix / Linux".
* W2-W5 [MUST] швидкі виграші: syscall-шлях, vDSO, `fork/exec`/`posix_spawn`,
  fault-around, slab із per-CPU кешами; lazy `PT_LOAD` + єдиний page-cache
  (збігається з W3-W5 ABI-роботою -- це одна задача).
* W5-W9 [MUST] BKL прибрано з гарячих шляхів; runqueue на CPU; VFS без
  глобального замка і ліниве дерево; [SHOULD] планувальник для інтерактивності,
  пакетні TLB shootdown.
* W7-W11 [SHOULD] NVMe/virtio-blk, NCQ, readahead, write-back; zero-copy.
* W9-W12 [SHOULD] tickless idle, швидке завантаження; [COULD] huge pages, io_uring.
* Вихід W12: усі рядки таблиці §6.6 виміряні; регресія > 5% від еталона блокує merge.

### W9-W11 (25.11-09.12) Оптимізація пам'яті і швидкості
* [MUST] Ледачий запуск сервісів (D-Bus activation + `hx-activate`, OpenRC для решти, §6.4).
* [MUST] zram (стиснена swap), pressure-based reclaim: при малій пам'яті скидати
  page-cache, стискати непроактивні вікна (`MADV_PAGEOUT`-аналог).
* [MUST] KSM-lite або принаймні dedup нульових/однакових сторінок для GTK.
* [MUST] Прогін `membench` для обох профілів, фіксація регресій у CI.
* [SHOULD] Профілі "Nook light" (<=70 МБ), "Nook full" (<=120 МБ),
  "XFCE" (<=200 МБ) -- різні набори сервісів за замовчуванням, вибір при інсталяції.
* [SHOULD] Прискорення завантаження: паралельний OpenRC (rc_parallel), prelink-подібне
  ущільнення відображень, зменшення initramfs.
* [COULD] Стиснення rootfs (hext + zstd) для економії page-cache.

### W10-W12 (02.12-23.12) Інтеграція обох DE
* [SHOULD] `xdg-desktop-portal` (легкий власний `hx-portal`): вибір файлів,
  скріншот, screencast (PipeWire-lite), відкриття URI -- працює в обох DE.
* [SHOULD] Мова: повний український переклад Nook + XFCE, розкладки, IME
  (`text-input-v3`).
* [COULD] HiDPI/дробове масштабування (fractional-scale) у Nook і XFCE.

### W12-W14 (16.12-31.12) Стабілізація і випуск 0.7 / 1.0-rc
* [MUST] Замороження функцій 16.12. Тільки виправлення й документація.
* [MUST] Прогін на реальному залізі (Samsung R428, ще 2-3 машини, Intel GMA/HD,
  AHCI, xHCI USB-клава/миша, Wi-Fi де можливо) -- список знайдених проблем.
* [MUST] Інсталятор оновлений: вибір DE (Nook/XFCE/обидва), профіль пам'яті.
* [MUST] Документація: `docs/DESKTOP.md` (Wayland/hxcomp), `docs/PORTING_GUIDE.md`
  ("як портувати софт: збираєш під musl, кладеш .desktop, все"), README, скриншоти
  Nook + XFCE, порівняльна таблиця пам'яті.
* [MUST] Реліз ISO 0.7.0 (x86_64) + образи aarch64/riscv64 (QEMU) зі зборкою Nook.
* [SHOULD] Публікація: сайт/репозиторій, короткі відео, пост "HamixOS vs Redox vs
  Alpine+sway: пам'ять, час старту".

## 6. Тулкіт для нативних апок

Варіанти: (1) власний `hx-toolkit` (швидкий, мінімальна пам'ять, повний контроль,
але писати віджети); (2) `iced`/`slint`/`egui` поверх Wayland (швидше писати,
більше RAM, залежності); (3) GTK4-rs (найбільша сумісність, найбільше RAM).
**Рекомендація:** `hx-toolkit` -- лише те, що вже використовують наші апки (кнопки,
списки, поля, вкладки, скрол), поверх спільного рендерера. Slint -- запасний варіант
для складних програм (hxsettings), якщо часу не вистачить.
"Нативний" тоді означає: Rust + Wayland + musl, а не "інший протокол".

### 6.1 Toolchain: відмова від hamix_std, стандартний musl [MUST, ADR-0005]

Що є зараз: `sdk/hamix_std` -- `#![no_std]` runtime на 3300 рядків (syscall-обгортки,
`brk`-алокатор, `println!`, `entry!`, `linux_bridge.rs`), від нього залежать 19
крейтів; збірка іде під кастомний target `kernel/x86_64-hamix_os.json` з nightly
(`-Zunstable-options`, `json-target-spec`). Це ті самі "два світи" на рівні мови:
свій `std`, якого немає в жодного стороннього крейта, і `linux_bridge`, що склеює
його з Linux ABI.

Рішення:
* Нативні програми збираються **стабільним Rust під `x86_64-unknown-linux-musl`,
  `aarch64-unknown-linux-musl`, `riscv64gc-unknown-linux-musl`** -- готові
  tier-2 target'и, зі справжнім `std`, потоками, `std::net`, `std::fs`, і тисячами
  крейтів з crates.io "просто працюють". Це знімає nightly, JSON-target і
  необхідність підтримувати власну стандартну бібліотеку.
* `sdk/hamix_std` видаляється поетапно: (1) W3: `hamix` -- нова тонка бібліотека
  лише для `SYS_HAMIX_*` (аудіо, дисплей, специфічні виклики), без алокатора й
  без `println!`; (2) W4-W8: міграція 19 крейтів разом з переходом на
  `hx-toolkit`; (3) W9: `hamix_std` і `linux_bridge` видалено.
* `sdk/hamix_kpi`/`hamix_linuxkpi` -- **лишаються**: це ABI модулів *ядра*, а не
  userland, до цього рішення вони не стосуються.
* Ядро й модулі й далі збираються під кастомний `x86_64-hamix_os` (`no_std`).

Як зберегти економію пам'яті та швидкість (без цього відмова була б помилкою):
* Профіль `release-min`: `opt-level="s"`, `lto="fat"`, `codegen-units=1`,
  `panic="abort"`, `strip`, `-C target-cpu` за профілем; `build-std` з
  `panic_immediate_abort` для найменших утиліт. Типовий Rust-hello на musl --
  ~300 КБ статично, RSS ~ сотні КБ.
* **Алокатор:** дефолтний mallocng musl повільний у багатопотоковому коді й
  тримає пам'ять. Для Rust-програм -- `#[global_allocator]` з малою arena
  (наприклад `mimalloc` у режимі малої пам'яті або власний `hx-alloc`) і
  `madvise(MADV_DONTNEED)` при простої; вимір у `membench`, обирає число, а не смак.
* **Статичне лінкування для нативних програм** (`+crt-static`): немає ld.so,
  немає символьного розв'язування на старті, швидший запуск. Чого це коштує:
  libc-код дублюється в кожному ELF -- це сотні КБ на програму, але резидентні
  лише торкнуті сторінки (lazy `PT_LOAD`, W3-W5), тож реальна ціна мала.
* **Динамічне лінкування -- для GTK/Qt/XFCE** (вони динамічні по суті) із
  єдиним `ld-musl` і спільним page-cache. Правило: `hx*` -- статично, чужий
  софт -- як зібрано.
* Не тягнути важке: `tokio`/`serde` на кожну дрібницю не додавати; бюджет
  на "hello"-вікно (<=1.5 МБ RSS) перевіряється в CI, тому роздування побачимо одразу.
* Швидкість: `fork/exec` -> `posix_spawn`/`vfork` у ядрі, швидкий `clone`,
  `vdso`-подібні `clock_gettime`/`gettimeofday` (зараз реальний syscall) --
  це компенсує те, що були надбудови в `hamix_std`.

Ризики: (1) `std` на musl тягне більше за `no_std` -- міряємо на `hxterm`, `hxfiles`
у W3 до масової міграції; (2) частину ядерних викликів, які використовував
`hamix_std` напряму, треба перевести на стандартні (Linux) еквіваленти -- це і є
"один ABI"; (3) треба, щоб ядро коректно віддавало `auxv`, TLS, `rseq`,
`set_tid_address`, `clone3`, потоки -- усе це вже в плані ядра W3-W5.

### 6.2 Майже повна відмова від `SYS_HAMIX_*` на користь Linux-викликів [MUST, ADR-0006]

Раніше в плані нативні виклики лишалися "як розширення". Це м'яко. Нове правило:
**Linux-syscall -- єдиний публічний ABI; ціль -- з 48 `SYS_HAMIX_*` лишити 0-5.**
Стан на 2026-09-24: 48 викликів (`kernel/src/syscall/native/mod.rs`, ~1200 рядків).
Майже всі мають прямий Linux-еквівалент або взагалі не мали б бути syscall'ами.

| Група `SYS_HAMIX_*` | Куди переїжджає |
|---|---|
| KILL, PIPE, SLEEP, REALTIME, STAT, READDIR, TRUNCATE, SYNC, MOUNT, UMOUNT, WAITPID, SPAWN, ISATTY, SET_FOREGROUND, SET_TERMINAL, TERMSIZE | `kill`, `pipe2`, `nanosleep`, `clock_gettime`, `statx`, `getdents64`, `ftruncate`, `sync`, `mount`, `umount2`, `wait4`, `posix_spawn`/`vfork+execve`, `ioctl(TCGETS)`, `tcsetpgrp`, `setsid`+`TIOCSCTTY`, `TIOCGWINSZ` |
| SHM_CREATE/MAP/RELEASE | `memfd_create` + `mmap` (це саме те, що вимагає Wayland `wl_shm`) |
| MSG_SEND/RECV, MAILBOX_FD, SERVICE_REGISTER/LOOKUP | `AF_UNIX` сокети в `/run/hx/*` (SCM_RIGHTS для передачі fd); реєстр сервісів = ім'я сокета |
| WAIT_EVENT, POLLKEY, READKEY, MOUSE | `epoll` + `/dev/input/event*` (evdev) і tty `read` |
| FBMAP, RELEASE_FB | `/dev/dri/card0` (DRM dumb buffer, `mmap`) як цільове; `/dev/fb0` як перехідне |
| SYSINFO, CPU_STATS, PROC_LIST, PROC_ALIVE | `sysinfo`, `/proc/{meminfo,stat,[pid]/*}`, `kill(pid,0)` |
| POWER | `reboot(2)` |
| TRACE | `ptrace` / `/proc/*/syscall` (`syscalltrace` стає звичайною утилітою) |
| DISK_LIST, DISK_IO, DISK_RESCAN | `/sys/block`, `/dev/sd*`, `ioctl(BLKRRPART)`, `read/write` на блочний пристрій |
| MKFS | не syscall: утиліта `mkfs.hext` у userland поверх блочного пристрою |
| AUTH, SET_OWN_PASSWORD, USERS_RELOAD | звичайні `/etc/passwd`+`/etc/shadow`, `setuid`-утиліти `su`/`passwd`/`login`, або `hx-authd` через AF_UNIX |
| CMD_LIST/REGISTER/RUN/UNREGISTER | не ядро: реєстр команд `hsh` -- це шлях `PATH` і вбудовані команди самої оболонки |

Що з цього випливає:
* Перелік до кінця W3 стає таблицею міграції в `docs/reference/syscalls.md`
  (виклик -> заміна -> статус). Кожен виклик проходить: реалізація Linux-шляху в
  ядрі -> перенесення споживачів -> позначка deprecated -> видалення.
* **Реалізація в ядрі -- головна вартість.** Особливо: evdev (`/dev/input`),
  DRM/`fb0` mmap, `AF_UNIX` з `SCM_RIGHTS` (обов'язково для Wayland), `memfd`,
  `/proc/[pid]/*`, `ioctl` для tty. Більшість потрібна і для Chromium/GTK/Qt, тож
  це не додаткова робота, а та сама, що в W3-W5.
* Залишок 0-5 (кандидати): лише те, чого Linux справді не вміє -- наприклад
  специфіка `hext` (знімки/відкат для pantry). Усе інше -- ні. Кожен залишений
  виклик потребує обґрунтування в ADR.
* **Продуктивність не страждає:** Linux-виклики -- ті самі переходи в ядро, а
  "швидкі" нативні шляхи зі списку (`SHM`, `MSG`) все одно дорожчі за
  `AF_UNIX` + `memfd`, які ядро вже реалізує. Найбільша виграш -- зникає
  дублювання: ядро підтримує один шлях коду замість двох.
* Порядок: ядерні Linux-шляхи (W3-W5) -> міграція `hsh`, `pantry`, `hxmon`
  (базова система) -> міграція `hx*` у W4-W8 -> прибирання (W9). Період
  подвійної підтримки -- один реліз, не більше.
* Тести: для кожного заміненого виклику -- тест, що порівнює поведінку зі
  специфікацією Linux (man 2), а не зі старою нативною поведінкою.

Що НЕ робимо: не копіюємо з Linux зайве (сам systemd, cgroups-v2,
namespaces, BPF) заради "повноти" -- лише те, що потрібне реальним програмам
і дає пам'яті/швидкості не менше, ніж давало нативне.

### 6.3 Nook після переходу на Wayland має виглядати і рухатись як зараз [MUST, ADR-0007]

Вимога: користувач не повинен помітити, що Nook переписано. Той самий вигляд
(панель, док, лаунчер, рамки вікон, тіні, іконки Nook Icons, світлий і темний
стилі), ті самі анімації (у `apps/hxserver/src/anim.rs`: ефекти open/close/
minimize/restore зі сплайнами `ease_out`/`smoothstep`, "лампа"-морфінг вікна
в док, settle, налаштовувані види й тривалості в `Prefs`, звуки з `sound.rs`).

Як це гарантуємо:
* **Спершу еталон, потім код (W1).** Знімаємо "золоту" референсну версію до
  жодних змін: набір скриншотів усіх станів (робочий стіл, панель, док із
  наведенням, лаунчер, меню, діалоги, обидва стилі) + послідовності кадрів
  кожної анімації при детермінованому часі (QEMU, фіксований 60 Гц, фіксований
  розмір 1280x800) + таблиця параметрів (тривалості, криві, розміри, відступи,
  кольори, шрифти) витягнута з коду. Це `tests/nook-golden/`.
* **Код не переписується з нуля, а переноситься.** Ефекти (`anim.rs`), WM
  (`wm.rs`), рендер (`render.rs`, `compositor/`) переїжджають у `hxcomp` майже
  без змін логіки; змінюється лише джерело вікон (Wayland-поверхні замість
  hxproto). Так візуальна поведінка зберігається "за побудовою".
* **Розподіл ролей:** ефекти вікон (лампа, fade, scale, тіні, рамки) -- у
  композиторі, бо потребують знімка вікна й контролю над кадром. Панель, док,
  лаунчер, робочий стіл -- Wayland-клієнти на `hx-toolkit` через layer-shell,
  із власним циклом анімації на frame-callback; анімація "вікно летить у
  іконку доку" залишається можливою через `foreign-toplevel` + геометрія
  іконки, яку док повідомляє композитору (аналог `dock_target`).
* **Автоматична перевірка:** скриншот-diff проти еталона (допуск <=1% пікселів
  на статичних сценах), порівняння покадрових послідовностей анімацій
  (тривалість і крива в межах 1 кадру), вимір часу кадру (не гірше за
  поточний). Розбіжність = червоний CI. Свідомі зміни дизайну оновлюють
  еталон окремим PR із поясненням.
* **Мінлива частина:** нативні вікна зберігають наявні рамки (server-side
  decoration через `xdg-decoration`); Linux-клієнти з CSD -- як зараз
  (`SetFrame`). XFCE-сесія має свій вигляд і не мусить копіювати Nook, окрім
  спільної теми й іконок.
* Налаштування анімацій у `hxsettings` (вид, тривалість, вимкнення) --
  лишаються з тими самими ключами конфігу; профіль "ecomode" лише додає
  вимкнення, а не змінює типові значення.
* Ризик: анімації залежать від гладкості кадру, а Wayland додає буферизацію.
  Контрміра: `frame`-callback + `presentation-time`, композитор не чекає
  повільних клієнтів (W2-W3), панель/док малює лише при зміні.

### 6.4 Система ініціалізації, джерело софту -- Alpine [MUST, ADR-0008]

**Виправлення попередньої версії цього розділу.** Спершу я виходив із того, що
`hxinit` -- користувацький PID 1, який можна навчити `sd_notify` тощо. Це
неправда: `hxinit` (`kernel/src/hxinit.rs`, 326 рядків) -- послідовник
завантажувальних юнітів *усередині ядра* (memory, pci, storage, rootfs...).
Користувацького менеджера сервісів у HamixOS зараз немає взагалі. Також стало
очевидно, що весь сторонній софт береться з **Alpine** (`pantry` уже читає Alpine
APKINDEX, ключі, `/opt/linux`). Це змінює висновки:

* **Alpine не використовує systemd.** Її пакунки зібрані без `libsystemd`; замість
  нього -- **OpenRC** (init), **elogind** (D-Bus `login1` і `libelogind`),
  **eudev** або `mdev`+`libudev-zero` (udev), **seatd**. Отже **емуляція systemd
  для Alpine-софту не потрібна**. Емулюємо/надаємо те, що Alpine справді очікує.
* Тому роботи менше, ніж у попередній версії: замість писати `hx-logind`,
  `hx-polkit`, UPower, UDisks2 з нуля -- беремо готові Alpine-пакунки, а власні
  заміни робимо лише там, де виміряно, що вони надто важкі.

**Рішення:**

| Шар | Що беремо | Примітки |
|---|---|---|
| Завантаження | **`hxinit` лишається** -- ядерний ранній init (pci, storage, rootfs, devices) | Від нього відмовляємось не ми, а від ідеї, що він же й керує сервісами. Кінець його роботи -- `execve` користувацького `/sbin/init` (OpenRC). Розділення: `hxinit` = "ядерний boot", OpenRC = "користувацькі сервіси" |
| PID 1 і сервіси | **OpenRC** (`openrc-init`, `openrc-run`, `supervise-daemon`, `rc-service`, `rc-update`) з Alpine | Скрипти `/etc/init.d/*`, `/etc/conf.d/*`, рівні `/etc/runlevels/*` з пакунків Alpine працюють без змін. Потрібен `busybox` (`sh`, `ash`) -- він уже в `provided` |
| Власні сервіси HamixOS | теж скрипти OpenRC (`hx-*` у `/etc/init.d`) | Один менеджер, один `rc-status`, один журнал |
| Логін/сесії/живлення | **elogind** (Alpine) + `libelogind`; `pam_elogind` за потреби | Дає `org.freedesktop.login1`, `XDG_RUNTIME_DIR`, inhibit, suspend. Своя заміна `hx-logind` -- лише якщо `membench` покаже перевищення бюджету |
| udev | `mdev` + `libudev-zero` (або `eudev`) + `hx-uevent` (netlink) | Без повного udevd, якщо вистачає |
| Політики/диски/живлення | Alpine `polkit`, `upower`, `udisks2` (збірка з elogind) | Спершу як є; профіль пам'яті вирішує, що замінити |
| Мережа | `hxnet` + (за потреби) NetworkManager-API-шим | як у §8 |
| D-Bus | Alpine `dbus`; D-Bus service activation як основа ледачого запуску | OpenRC socket activation не вміє; для не-D-Bus сервісів -- крихітний `hx-activate` (inetd-подібний) |

**Що робити з "емуляцією" двох систем:**
1. **OpenRC -- реальний, не емуляція.** Це готовий маленький C-проєкт під musl,
   що запускається як є; ми лише переконуємось, що ядро дає йому все потрібне
   (`/run`, `mount`, `pivot`/`switch_root`, `setsid`, `signalfd`/`kill`, `/proc`,
   `/sys/fs/cgroup` порожній, `openrc-init` як PID 1, `reboot(2)`, `sysfs`).
2. **systemd -- не емулюємо**, бо його немає ні в Alpine, ні в наших пакунках.
   Виняток (низький пріоритет, W11+): якщо колись знадобиться сторонній
   бінарник із `libsystemd`, додаємо `libsystemd`-shim, що нічого не робить
   для `sd_notify` і повертає `ENOENT`/`ENOTSUP` для решти. Не обіцяємо.
3. Корисно, щоб `hxctl` (з §5.0) був тонкою обгорткою над `rc-service`/
   `rc-status`: одна команда для людини, під капотом реальний OpenRC.

**Пакунки і власні репозиторії ("щоб не морочитись потім"):**
* Формат пакунків HamixOS -- **apk** (той самий, що в Alpine), рецепти --
  **`APKBUILD`** для `abuild`. Тоді власні пакунки (`hx*`, `nook-*`,
  `hxcomp`) збираються так само, як Alpine, кладуться у власний репозиторій
  (`hamix-main`, `hamix-community`) зі своїм підписним ключем, а чужі
  залишаються з Alpine без переробок. Патчі до апстріму -- у нашому форку
  `aports`, відстежуваному як різниця.
* `packages/<name>/manifest.toml` з §5.0 замінюється на `packages/<name>/APKBUILD`.
* **`pantry` лишається фронтендом**, але його "Alpine-стиль" стає єдиним:
  зникає `/opt/linux`, `apk`-пакунки розпаковуються прямо в `/`; тригери
  (`.post-install`, `.trigger`), `alpine-baselayout`, `busybox` -- з Alpine.
  Рішення "лишити pantry чи перейти на справжній `apk-tools`" -- окремий ADR
  до W4; за замовчуванням лишаємо pantry, поки він не відстає по функціях.
* **Розклад ФС береться з Alpine на пінованому релізі** (`/etc/init.d`,
  `/etc/conf.d`, `/etc/runlevels`, `/var/lib/apk`-подібна база), а не вигадується
  окремо. Шляхи `/usr/lib/hxinit/services` з §5.0 замінюються цим. Зверніть
  увагу: Alpine історично не "merged-usr" (`/bin`, `/lib` окремо) -- дотримуємось
  цього на пінованому релізі, щоб пакунки не конфліктували.
* **Пін релізу Alpine** (наприклад v3.2x) -- один на весь реліз HamixOS;
  оновлення = перехід на наступний реліз, як у Alpine, зі звіркою `aports`.

**Пам'ять і швидкість:** OpenRC швидкий і малий (`openrc-init` + `supervise-daemon`
у сумі <1 МБ RSS у простої, скрипти живуть лише під час запуску); `busybox`
спільний. Ціна elogind (кілька МБ) записується в бюджет §3 і міряється --
якщо виходить за межі, замінюємо на `hx-logind` з тим самим D-Bus API.
Ледачий старт (з §3) реалізуємо через D-Bus activation + `hx-activate`, а не
через socket activation systemd.

**Порядок і терміни:**
* W1-W2: ADR-0008; зафіксувати реліз Alpine; інвентар: які Alpine-пакунки
  потрібні для Nook і XFCE, які з них залежать від elogind/eudev/polkit.
* W3-W4: **деталь PID:** `hxinit` зараз потік ядра з pid 1, а `openrc-init` перевіряє
  `getpid()==1` (як і багато програм). Рішення: нумерація pid у ядрі так, щоб
  перший користувацький процес мав pid 1 (потоки ядра -- окремий простір/негативні id).
  Заодно можна перейменувати модуль на `hxboot`, щоб не плутати (W3, косметика).
* W3-W4: ядро готове для OpenRC (`openrc-init` як PID 1, `/run`, `/sys`, tty,
  `reboot`); `hxinit` передає керування; власні `hx-*` сервіси як OpenRC-скрипти.
* W4-W6: elogind + libudev-zero/mdev + dbus; `XDG_RUNTIME_DIR` через elogind;
  `hx-session` як сесія під elogind.
* W5-W7: перехід `pantry` і формату на `apk`+`APKBUILD`; власний репозиторій
  `hamix-main` зі ключем; прибирання `/opt/linux`.
* W6-W9: polkit/upower/udisks2 з Alpine, перевірка з XFCE-сесією.
* W9-W11: `membench`, заміна важких частин; `libsystemd`-shim -- лише якщо треба.
* Критерій: `rc-status` показує сервіси; XFCE-сесія вимикає/перезавантажує/
  присипляє/блокує; Thunar монтує диск; `apk`-пакунки з Alpine, що приносять
  `/etc/init.d/*`, встановлюються й стартують без ручного втручання;
  `abuild` збирає наш `hxcomp` і кладе його у власний репозиторій.

Чого не робимо: не портуємо systemd; не пишемо власний менеджер сервісів
(відхилено -- OpenRC вже робить це і сумісний із пакунками Alpine).

### 6.5 Mesa і апаратне прискорення через KPI [MUST/SHOULD, ADR-0009]

Стан: у HamixOS немає DRM. Є власний клас "display" у KPI (`DisplayOps`,
`KPI_VERSION = 6`, `docs/roadmap/KPI_ROADMAP.md`): конектори, EDID, режими, page-flip,
курсорна площина, `flush`. `CAP_FILL/CAP_COPY` не мають жодного бекенду
(virtio-gpu 2D не малює). GPU-модулі -- `virtio-gpu` і `intel-display` (лише
дисплей, без 3D). Mesa же спілкується з ядром **тільки** через Linux DRM UAPI:
`/dev/dri/card0` і `renderD128`, ioctl-и `DRM_IOCTL_*`, GEM-хендли, PRIME/dma-buf,
syncobj, sysfs `/sys/class/drm`, PCI-ідентифікація. Отже треба новий шар.

Принцип (той самий, що весь план): **не портувати DRM-ядро Linux**, а зробити
в ядрі тонкий шар, який говорить Linux DRM UAPI, а всередині спирається на
наш KPI. Модулі (`virtio-gpu`, `intel-display`, пізніше інші) реєструють у ньому
свої можливості, як реєструються зараз у display-класі.

Що додається (KPI v7, нова група "drm"):
* `/dev/dri/card0` (KMS) і `/dev/dri/renderD128` (render node) зі статичним
  sysfs (`/sys/class/drm/card0*`, `/sys/dev/char/226:*`, PCI vendor/device) --
  `libdrm` і Mesa знаходять пристрій без udev-магії.
* **KMS-частина:** об'єкти CRTC/plane/connector/encoder, dumb-буфери, `page_flip`
  з подіями vblank, атомарний commit у мінімальному вигляді, курсорна площина.
  Це те, чим користуватиметься `hxcomp` замість `FBMAP` (закриває §6.2).
* **GEM/PRIME:** хендли буферів, mmap-offset, експорт/імпорт dma-buf як fd
  (потрібно для Wayland `linux-dmabuf` -- тому що вона вже в W2-W3 плані).
  Увага з `hamix-lazy-memory`: великі буфери йдуть через vmalloc і **не
  фізично суцільні**, тож модуль віддає сторінковий список (scatter-gather), а
  не адресу -- virtio-gpu це так і хоче.
* **Синхронізація:** fences і `syncobj` (binary спершу, timeline пізніше).
* **Що потрібне модулю від ядра:** MSI/MSI-X, відображення BAR, DMA API,
  завантаження прошивки (`/lib/firmware`), power-management хуки. Ліміт
  `BUDGET_BYTES = 20 МіБ` залишається для коду модулів; GEM-пам'ять рахується
  окремо й контролюється композитором/`membench`.
* Тести: `modetest`, `kmscube`, `drm_info`, власні тести UAPI.

Сходи прискорення (кожна сходинка самостійно корисна):

| # | Що | Драйвер Mesa | Де перевіряти | Строк |
|---|---|---|---|---|
| 0 | Програмний GL/Vulkan: `llvmpipe`/`softpipe`/`lavapipe` на dumb-буферах (`kms_swrast`) | swrast | QEMU, будь-яке залізо | W4-W7 [MUST] |
| 1 | **virtio-gpu 3D (virgl)**: GL через хост | `virgl` | QEMU `-device virtio-vga-gl -display gtk,gl=on` (потрібен GPU хоста) | W6-W10 [MUST] |
| 2 | Vulkan у VM (Venus) | `venus` | QEMU | W10-W12 [COULD], далі 2027 |
| 3 | **Intel**: підмножина i915/xe UAPI під Mesa `iris`/`crocus` | `iris` (Gen8+) | реальне залізо | розвідка W10-W12 [SHOULD], реалізація 2027 |
| 4 | AMD/NVIDIA | radeonsi/nouveau | -- | поза планом 2026 |

Чесно про Intel: `intel-display` уже вміє режими/EDID/page-flip, але для Mesa
потрібні GEM, контексти, `execbuffer`, VM/GTT, GuC/HuC-прошивки на нових
поколіннях, скидання GPU. Це місяці роботи на *одне покоління*. У W10-W12
робимо лише розвідку: яке покоління ціль (не Gen4 з R428 -- для нього Mesa
`i965` застарів і прискорення нереалістичне), скільки коду, ADR. Реалізація --
2027. Перший реально "апаратний" GPU-результат 2026 року -- virtio-gpu/virgl у VM,
не залізо.

**Прискорення інтерфейсу не залежить лише від Mesa.** Дешеві виграші через KMS
без жодного GL:
* курсорна площина (вже є) і **оверлейні площини** для відео (`hxvideo`) і
  для панелі/дока -- менше композитингу на CPU;
* **direct scanout** повноекранного вікна (гра/відео) без копії;
* нормальні `page_flip` + vblank, не малювати зайве (damage tracking, W2-W3).

**GPU-композитинг у `hxcomp`** -- необов'язковий бекенд:
* Основа лишається CPU-рендер (тайли, SIMD): вона тримає бюджет 70 МБ.
* Опційний GLES2-бекенд (через EGL+GBM або власний легкий шар) вмикається
  лише на пристроях із прискоренням і профілем "GPU": ефекти Nook (лампа,
  тіні, blur) переходять на GPU, зростає плавність і падає навантаження на
  CPU/акумулятор. Візуальний результат має збігатися з еталоном (§6.3).
* Бюджет пам'яті профілю "GPU": +<=25 МБ до базового (libEGL/libgallium/virgl,
  шейдер-кеш). У Nook-профілі 70 МБ Mesa **не завантажується**, доки жоден GL-клієнт
  або GPU-бекенд не запущено.
* **Mesa із Alpine** (`mesa-dri-gallium`, `mesa-egl`, `mesa-gles`, `mesa-gbm`,
  `libdrm`, `mesa-vulkan-*`) -- без власної збірки. Небезпека: `llvmpipe`
  тягне LLVM (десятки МБ RSS), тому для програмного шляху за замовчуванням --
  без GL у легких програмах (GTK4 з `GSK_RENDERER=cairo`, Qt software,
  Chromium `--disable-gpu`), а `llvmpipe` ставиться лише за потреби.

Клієнти, які це використовують: GTK4 (GSK ngl), Qt (EGL), Firefox WebRender,
Chromium (GPU raster), mpv (`gpu`), `xfce4-*` (GTK). Критерій кожної сходинки --
конкретна програма: `kmscube` -> `weston-simple-egl` -> `glmark2-es2-wayland` ->
GTK4 із GL-рендерером -> Firefox.

Ризики: (1) обсяг DRM UAPI, який вимагає Mesa, більший за очікуваний -- вирізаємо
лише те, що викликає конкретний драйвер (virgl потребує небагато); (2) `virgl`
у QEMU потребує GL/GPU на хості розробника -- запасний варіант: `llvmpipe` у CI;
(3) шейдер-кеші й Mesa можуть з'їсти пам'ять -- лише в профілі "GPU";
(4) fence/sync-помилки дають завислі кадри -- тайм-аути й рестарт бекенду.

### 6.6 Швидкість ядра: перероблюємо за виміром, а не за відчуттям [MUST/SHOULD, ADR-0011]

Ціль: "дуже швидко" має числа. Чесно про очікування: з малим ядром без
десятиліть спадщини реально **зрівнятися з Linux на мікробенчмарках** і
**перевершити** його на старті, затримці вводу/кадру й розмірі -- але не
"в рази швидше в усьому". Тому мета сформульована так: не гірше за Alpine Linux
у тому самому QEMU/KVM на типових операціях, і краще -- на завантаженні,
затримці інтерфейсу й пам'яті.

**Діагноз поточного ядра (з коду, 2026-09-25):**
* один глобальний `SCHED: Mutex<Sched>` (`kernel/src/task/mod.rs:428`) --
  усі CPU ставлять і беруть задачі через один замок;
* глобальний "великий замок ядра" `task/bkl.rs` (BKL);
* VFS -- один `VfsLock(Mutex<Option<Vfs>>)`, уся ФС -- дерево в пам'яті
  (`Vec<Node>` + `BTreeMap<String,usize>` на директорію, ~15,7 тис. вузлів);
  кожна операція над файлами серіалізується;
* файловий кеш і mmap не з'єднані в один page-cache (тому `execve` читає весь
  бінарник у купу -- див. §5 W3-W5);
* блочний ввід/вивід -- лише AHCI/IDE, без NVMe; нема readahead/writeback;
* багато `spin::Mutex` замість чогось, що не б'є по всіх ядрах;
* `MAX_CPUS` масиви статично (це нормально), купа ядра росте шматками по 2 МБ.

**Принцип:** жодної оптимізації без бенчмарка, що її доводить, і без стрес-тесту,
що не зламано коректність. З нотатки про блокування відомо, що ця частина
ядра вже давала зависання всієї системи (`hamix-kernel-locking`), тому
безпека -- умова, а не побічний ефект.

**Крок 0: інструменти (W1-W2) [MUST].**
* `tools/kbench` -- набір як lmbench/will-it-scale у QEMU: пусте syscall,
  `getpid`, `read`/`write` на pipe, context switch, `fork+exec+wait`,
  `mmap`/page-fault, `open/stat/close`, послідовне і випадкове читання файлу,
  створення 10 тис. файлів, `clone` потоків, `futex` ping-pong.
* Ті самі тести запускаються на Alpine Linux у тому ж QEMU -- це еталон "1.0".
* Профайлер `hxprof`: вибірка за таймером (лічильник RIP + стек) з
  агрегацією, `/proc/hxprof`; лічильники контенції замків (`bkl.rs` вже має
  `CONTENDED`) для кожного глобального замка.
* Вимірювання інтерфейсу: затримка "рух миші -> зміна кадру", час кадру
  композитора (P50/P99), час запуску `hxterm`, завантаження до робочого столу.
* Виконується щодня в CI і пише число в таблицю, як `membench`.
* **Кодова безпека:** debug-режим із перевіркою порядку замків (lockdep-lite:
  `VFS -> BACKENDS -> ...`), тайм-аут на утримання замка, стрес-тест
  `-smp 4` із випадковими затримками, що ловить дедлок/лайвлок до релізу.

**Швидкі виграші (W2-W5) [MUST]** -- зазвичай дають більше за глибокі переробки:
* Швидкий шлях syscall (`SYSCALL`/`SYSRET` без зайвих збережень, без
  `alloc` у гарячому шляху, перевірка user-указників без копіювання рядків);
  vDSO (`clock_gettime`, `gettimeofday`, `getcpu`) -- зараз це справжній syscall.
* `fork/exec`: `vfork`/`posix_spawn` без копіювання адресного простору,
  COW-`fork`, не читати весь ELF (lazy `PT_LOAD`), спільні бібліотеки з page-cache.
* Page fault: пакетна "fault-around" для file-backed mmap (мапити 16 сторінок),
  zero-page, `MAP_POPULATE`.
* Алокатори: slab із per-CPU кешами для гарячих об'єктів (файли, задачі,
  сторінки), швидкий шлях кадрів без глобального замка.

**Масштабованість (W5-W9) [MUST для першого, SHOULD для решти]:**
1. **Прибрати BKL** з шляхів, де без нього можна (`task/bkl.rs`): кожне місце,
   де він потрібен, має конкретну причину, записану в ADR (не в коментарях коду); решта під власні
   замки. [MUST]
2. **Черга виконання на CPU** замість глобального `SCHED`: локальні
   runqueue + балансування (work stealing), `NEED_RESCHED` уже per-CPU. [MUST]
3. **VFS без глобального замка:** dentry/inode-кеш із шардованими замками або
   RCU-подібним читанням; шлях пошуку (`walk`) без блокування на читання;
   ліниве завантаження вузлів із hext замість повного дерева в пам'яті
   (це ще й економія пам'яті). [MUST]
4. **Єдиний page-cache** для `read/write`, `mmap`, `execve` (спільний з
   §5 W3-W5): одна копія сторінки, зворотне відображення, скидання при
   нестачі пам'яті. [MUST]
5. Розумніший планувальник: інтерактивні задачі (композитор, введення,
   аудіо) отримують малу затримку -- "EEVDF-lite" або пріоритетні класи + 
   `SCHED_FIFO`/nice для композитора. [SHOULD]
6. Знижена синхронізація TLB: пакетні shootdown, PCID, `INVLPG` лише де треба. [SHOULD]
7. Великі сторінки (2 МБ) для купи ядра й великих анонімних mmap
   (прозоро, з лімітом фрагментації). [COULD]

**Ввід/вивід (W7-W11) [SHOULD]:**
* AHCI: NCQ, асинхронні черги, MSI; **NVMe-модуль** (KPI) -- без нього швидкі
  сучасні диски не використати; virtio-blk/virtio-scsi для VM. [MUST для NVMe або virtio-blk]
* Readahead, write-back із коалесцією і `fsync`, що справді чекає; `hext` -- копіювання при записі
  та контрольні суми без зайвих проходів.
* Zero-copy: `sendfile`, `splice`, `copy_file_range`, `memfd`/dma-buf для Wayland.
* Мережа: `smoltcp` -- перевірити копіювання буферів, GRO/TSO де є, `epoll` без O(n).
* `io_uring` (підмножина) -- лише якщо тест покаже вузьке місце. [COULD]

**Затримка й енергія [SHOULD]:**
* Безтіковий простій (tickless idle), об'єднання таймерів, `hlt`/`mwait`.
* Швидке завантаження: паралельні юніти, менший образ, `hext` без повного
  сканування на старті, ціль <=6 с до робочого столу (див. §4).

**Цілі (перевіряються `kbench` щодня, порівняння з Alpine Linux у тому самому QEMU/KVM):**

| Метрика | Ціль |
|---|---|
| пусте syscall / `getpid` | <= 1.0x Linux |
| context switch, pipe ping-pong | <= 1.2x Linux |
| `fork+exec+wait` невеликої програми | <= 1.0x Linux |
| page fault, `mmap` файлу | <= 1.2x Linux |
| `open/stat/close`, створення файлів | <= 1.5x Linux (VFS-переробка знижує) |
| читання файлу з page-cache | <= 1.1x Linux |
| час завантаження до робочого столу | <= 0.5x Alpine+XFCE (ціль <=6 с) |
| затримка вводу -> кадр (Nook) | P99 <= 2 кадри |
| резидентна пам'ять ядра (Nook-профіль) | <= 22 МБ |

**Ризики:** (1) багатоядерні переробки дестабілізують ядро -- CI зі стресом `-smp 4`
і lockdep-lite обов'язкові до merge; (2) швидкі структури (per-CPU кеші, шарди)
коштують пам'яті -- перевіряти `membench`, кеші малі й адаптивні; (3) занадто
сильна оптимізація перед завершенням ABI (W3-W5) зламає роботи, що зачіпають ті
самі файли -- тому глибокі зміни VFS/планувальника йдуть після lazy PT_LOAD і
page-cache; (4) не перетворювати на змагання з Linux у всьому -- цілі в таблиці, решта не гонитва.

## 7. Ризики і що робити

| Ризик | Ймовірність | План Б |
|-------|-------------|--------|
| Chromium/Firefox не запускаються до W5 | висока | Не блокує Nook/XFCE; лишити Telegram/Falkon/Qt-WebEngine-lite; Chromium перенести на 2027 |
| GTK4/GTK3 тягнуть забагато RAM | середня | Ставка на GTK3+XFCE 4.18/4.20, профіль zram, вимкнення GVFS/a11y |
| Переписування Nook ламає стабільність | висока | Паралельний режим hxproto до W7; регресійний QEMU-набір; feature-freeze 16.12 |
| XFCE 4.22 з Wayland вийде пізніше за грудень | середня | Пінований dev-снепшот 4.21.x як "preview"; аварійний X11-сеанс; реліз 0.7 не блокуємо |
| Layer-shell/foreign-toplevel/output-management зайняли більше часу | середня | Пріоритет цим протоколам у W6-W9; мінімальна підмножина для панелі й меню |
| 200 МБ з XFCE не вдається | середня | Оголосити 250 МБ для 0.7, 200 -- для 0.8; чесна метрика в README |
| Nook після перепису виглядає/рухається інакше | середня | Золотий еталон з W1, скриншот-diff і покадрове порівняння анімацій у CI; код переноситься, а не пишеться наново |
| Софт вимагає logind/udev/polkit/UPower/UDisks2 | середня | Готові Alpine-пакунки (elogind, libudev-zero, polkit, upower, udisks2); власні заміни лише за результатом `membench` (§6.4) |
| Alpine-пакунки не працюють без OpenRC-шарів або мають зв'язки з systemd | середня | Реальний OpenRC; `libsystemd`-shim як запасний варіант; пінований реліз Alpine |
| Обсяг DRM/Mesa більший за 14 тижнів; Intel-3D нереальний у 2026 | висока | Реалізація лише virgl у VM + swrast; Intel лише розвідка; KMS-виграші (планки, direct scanout) не залежать від Mesa |
| Переробка планувальника/VFS/BKL дестабілізує ядро (вже були фризи) | висока | Спершу `kbench`+lockdep-lite+стрес `-smp 4`; зміни малими кроками; глибокі зміни після page-cache; відкат за прапором |
| Реальне залізо ламається (драйвери) | висока | Реліз орієнтується на QEMU + перелік перевіреного заліза; драйвери -- окрема гілка |
| Переоцінка обсягу | дуже висока | Правило MUST/SHOULD/COULD, тижневий огляд (п'ятниця), відсікати COULD першими |

## 8. Що ще варто додати (мої пропозиції за межі запиту)

1. **Pantry як повноцінний менеджер пакунків:** підписані репозиторії, атомарні
   оновлення (A/B-розділи або hext-знімки) з відкатом. Це головна "фіча ОС" після
   DE. [SHOULD, кінець 2026 / початок 2027]
2. **Самохост:** `rustc` + `cargo` + `git` на HamixOS, зібрати ядро зсередини ОС.
   Найсильніший пункт у порівнянні з Redox. [COULD 2026, ціль Q1 2027]
3. **Пісочниця застосунків** за `docs/architecture/PERMISSIONS.md`: Wayland-клієнт отримує тільки
   свої fd; `xdg-portal` для файлів/екрану; seccomp-подібні фільтри. Безпека
   Wayland-моделі стає реальною перевагою. [SHOULD]
4. **Живий образ + USB-встановлення** з одним ISO для x86_64 UEFI/BIOS,
   `hxinstall` з розбиттям диска і шифруванням (dm-crypt-lite). [MUST для 0.7]
5. **Звук:** PipeWire-сумісний шар поверх `hxsound` (щоб GTK/Chromium/Telegram/mpv
   мали аудіо без pulseaudio-демона). [SHOULD]
6. **Мережа:** NetworkManager-сумісний D-Bus API (мінімальний) -- потрібен
   XFCE-аплету, GNOME-програмам і Telegram/GLib (`GNetworkMonitor`). Wi-Fi
   `crates/wlan` -> wpa_supplicant-сумісний інтерфейс. [SHOULD]
7. **Живлення/ноутбуки:** ACPI battery/AC, suspend-to-RAM (S3), backlight, lid --
   без цього ОС "не для щоденного використання" на ноутбуці. [SHOULD]
8. **GPU-прискорення:** Mesa `virgl`/`venus` для virtio-gpu, потім `i915`-модуль
   (є `intel-display`). Спершу llvmpipe з обмеженням потоків. [COULD]
9. **CI:** GitHub Actions + KVM-QEMU runner: збірка, boot-тест, скриншот-diff
   робочого столу, `membench`. Скриншот-diff ловить регресії UI автоматично. [MUST]
10. **Доступність (a11y):** мінімум -- масштаб, високий контраст, великий курсор,
    екранна клавіатура (`text-input-v3`); AT-SPI пізніше. [COULD]
11. **Ігри/мультимедіа:** `hxvideo` (H.264/AAC вже є) -> Wayland + VA-подібне
    прискорення нема; майбутній фокус -- `mpv`+`ffmpeg` як "нормальні" пакунки. [COULD]
12. **Спільнота:** `docs/PORTING_GUIDE.md`, сторінка "Що працює" (application
    compatibility list), шаблони `.desktop` і pantry-рецептів. Це прискорить
    портування зовнішніми людьми. [SHOULD]

## 9. Вихідні критерії (Definition of Done для 2026)

* Nook: Wayland-композитор + shell-клієнти, `hxproto` видалено.
* Усі нативні апки -- Wayland-клієнти на `hx-toolkit`.
* Ядро: `kbench` щодня, усі цілі таблиці §6.6 виміряні, немає регресій відносно еталона Alpine понад 5%; BKL не в гарячих шляхах, runqueue на CPU, VFS без глобального замка.
* DRM/KMS-шар і Mesa: `kmscube` і `glmark2-es2-wayland` працюють у QEMU (swrast + virgl); Nook рендерить через KMS; Mesa не вантажиться в базовому профілі, §6.5.
* Nook пройшов порівняння з золотим еталоном (вигляд і анімації), §6.3.
* OpenRC як єдиний менеджер сервісів; пакунки Alpine з `/etc/init.d/*` працюють; власний apk-репозиторій із `abuild`; XFCE-сесія користується elogind/udev/polkit/UPower/UDisks2, §6.4.
* XFCE-Wayland-сеанс (реліз 4.22 або пінований dev-снепшот) стабільний, вибір DE в екрані входу.
* Idle: Nook <=70 МБ, Nook full <=120 МБ, XFCE <=200 МБ (або чесно <=250 МБ, якщо
  тест показує; мета 200 залишається на 0.8) -- вимірювано `membench` за §4.
* Працюють: Telegram, GTK3/4-програми, Qt6-програми, Firefox або Chromium (мін. одне),
  mpv, Thunar, xfce4-terminal.
* CI збирає ISO, запускає boot-тест і скриншот-порівняння.
* Реліз 0.7.0 з новим README, документацією і порівнянням із Redox.

## 10. Робочий ритм

* Пн: планування тижня, п'ятниця: `membench` + огляд ризиків + оновлення цього файлу.
* Кожна велика ділянка (композитор, ABI, XFCE, пам'ять) -- окрема гілка, злиття в
  `main` тільки при зеленому регресійному наборі.
* Ніяких нових фіч у shell/DE після 16.12.
