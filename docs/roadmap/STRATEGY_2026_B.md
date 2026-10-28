# HamixOS: стратегія B до кінця 2026 ("Своє ядро, своя libc, Linux як шар")

Дата складання: 2026-09-25. Дедлайн: 2026-12-31 (~14 тижнів, W1..W14).
Це **альтернатива** `docs/roadmap/STRATEGY_2026.md` (далі -- план A), а не заміна: план A
лишається без змін для порівняння. Обирати між ними -- вам (див. §14).

## 0. Суть в одному абзаці

HamixOS -- **самодостатня власна ОС**: власне ядро, власна libc (**hxlibc**),
власний ABI, власний дисплейний сервер (**hxserver/hxproto**), власна DE (**Nook**),
власний init, власна файлова система (**hext**), власні драйвери (KPI). Linux-софт
працює через **hxlinuxulator** -- необов'язковий шар сумісності: Linux-програми
лінкуються з **musl**, нативні -- з **hxlibc**, а система їх не розрізняє нічим,
крім того, яку "особистість" (personality) отримує процес. Шар вимикається **однією
дією**, після чого нічого не ламається, бо ніщо нативне від нього не залежить.

Відмінність від плану A: там нативне поступово розчинялось у Linux ABI
(`SYS_HAMIX_*` -> 0-5, `hamix_std` -> musl, Nook -> Wayland). Тут навпаки:
**нативне ростить і лишається головним**, Linux -- гість у тому ж домі.

## 1. Що в HamixOS реально своє (і що ми будемо розробляти самі)

| Шар | Компонент | Стан | Що робимо у 2026 |
|---|---|---|---|
| Ядро | Rust-ядро, hext, KPI-модулі, ленива пам'ять | є | швидкість (§10), NVMe/virtio-blk, page-cache |
| **libc** | **hxlibc** = форк **mlibc** з Hamix-бекендом (§4A) | -- (є `hamix_std`, 3300 рядків) | **головний великий проєкт року** (§4A) |
| ABI | **Hamix ABI v1** (`SYS_HAMIX_*`) | є, 48 викликів, неописаний | аудит, версія, документ, розширення (§5) |
| Вікна | **hxserver + hxproto + hxclient** | є | **розвиток** (hxproto v2), а не заміна (§7) |
| DE | **Nook** | є | розвиток, вигляд і анімації незмінні (§7) |
| Init | **hxinit** + user-level `hxsvc` | ядерний boot є | супервізор сервісів свій (§9) |
| Пакунки | **pantry** + власний репозиторій | є (Alpine) | шари `native` / `linux` (§6.5) |
| Тулкіт | **hx-toolkit** (нативні віджети) | частково в hxclient | ростить (§7) |
| Компілятор | `hx-cc` (clang + hxlibc) | -- | C-програми нативно (§4) |
| Драйвери | KPI-модулі | є | virtio-gpu 3D, NVMe (§10) |
| Сумісність | **hxlinuxulator** + hxwayland | є, ~7,2 тис. рядків | ізоляція й перемикач (§6) |

Головна відповідь на "нам нічого самим розробляти": **тут власних великих
систем шість** -- ядро, hxlibc, hxserver/Nook, hxinit/hxsvc, hext, pantry-репозиторій --
а Linux лишається інструментом сумісності, а не основою.

## 2. Архітектура: два шари, одне оточення

```
   нативні програми (Rust/C на hxlibc)           Linux-програми (musl / Alpine)
   hxterm, hxfiles, hxnotes, Nook-shell ...      Telegram, Firefox, GTK/Qt, XFCE ...
            |  hxproto (вікна, події)                 |  wayland / X11
            |                                     hxwayland + Xwayland  (шар Linux)
            +------------------ hxserver / Nook ------------------+
            |  Hamix ABI (SYS_HAMIX_*)                            |  Linux ABI
            |                                                     |  hxlinuxulator (personality)
 ==========================  ЯДРО HamixOS (ядро одне)  =============================
        планувальник, пам'ять, hext, VFS, IPC, мережа, KPI-драйвери
```

Один корінь, одна файлова система, один процесний простір, один `ps`, один
пакетний менеджер. Різниця лише в **особистості процесу**, яку ядро визначає з
ELF-заголовка при `execve` (див. §6.2).

## 3. Інваріанти (правила, які тримають ідею)

1. **Правило листа.** Нативний код НІКОЛИ не залежить від шару Linux. Залежності
   йдуть лише в один бік: Linux-шар -> нативне. Перевіряється в CI: пакунки
   мають мітку `layer = native | linux` (pantry), нативний пакунок з
   залежністю на `linux` -- помилка збірки.
2. **Образ `linux=off` -- повноцінний.** У CI щодня збирається і завантажується
   образ без шару Linux, на ньому проходить **весь нативний набір тестів**
   (Nook, усі `hx*`-програми, hsh, pantry, мережа, звук, відео, інсталятор).
   Червоний тест = стоп merge.
3. **Одна файлова система.** Ніяких `/opt/linux`, `/proc/linux`. Пакунки Linux
   встановлюються в звичайні місця (`/usr`, `/etc`, `/lib`) і мають власника
   в pantry. Простір імен не розділяємо -- розділяємо *власників і особистості*.
4. **Кожна нативна можливість -- нативний виклик першою** (так було в
   `ARCHITECTURE_ROADMAP.md`, п.1: правило зберігається). Linux-версія --
   окрема, явно позначена робота, що будується *над* нативною.
5. **Бюджет пам'яті рахується окремо для шару:** "Nook без Linux" <=70 МБ;
   Linux-шар не завантажений, доки не запущено першу Linux-програму (§6.3).

## 4. hxlibc: власна libc [MUST, ADR-0005B]

**Що це:** libc, написана на Rust, з C-ABI (`libhxc`) і власним нативним API
поверх Hamix ABI. Для нативних програм -- те, чим є musl для Linux-програм.
"У Linux -- musl, у HamixOS -- hxlibc."

**Чому це варте зусиль:** це головний "великий і свій" компонент; усі нативні
програми залежать від нього, ми контролюємо пам'ять, швидкодію й безпеку;
можна робити те, чого музл не вміє (першокласні порти/сервіси/shm/дисплей,
гарантії ледачого ініціалізування, нуль накладних витрат на старті).

**Чесно про обсяг:** повна POSIX-libc -- роки роботи (у Redox `relibc` --
багаторічний проєкт). Тому hxlibc росте **шарами**, кожен вже корисний:

| Шар | Що всередині | Коли |
|---|---|---|
| `hxlibc-sys` | сирі syscall-обгортки Hamix ABI (те, що зараз у `hamix_std::sys`) | W2-W3 |
| `hxlibc-core` | алокатор (`malloc/free/realloc`, малі arena), потоки й TLS, `futex`-примітиви, час, env, `errno` | W2-W5 |
| `hxlibc` (C ABI) | `stdio`, `string`, `stdlib`, `unistd`, `fcntl`, `dirent`, `time`, `pthread`, `signal` (підмножина), сокети, `dlopen`-заглушка; заголовки `<stdio.h>`... | W4-W9 |
| `hx-cc` | обгортка clang: `--target=x86_64-unknown-hamix`, `crt0/crti/crtn`, лінкер-скрипт, sysroot | W4-W7 |
| `hxstd` (Rust) | те, що зараз `hamix_std`, але зверху на hxlibc; **API збережений**, 19 крейтів мігрують без переписування | W3-W8 |
| Rust `std` для `x86_64-unknown-hamix` | форк бібліотеки через `-Zbuild-std`, `libc`-крейт для Hamix | W9-W12 [SHOULD], далі 2027 |
| динамічне лінкування (`ld.hx`, `libhxc.so`) | спільна libc між програмами | 2027 |

**Правила дизайну (дають ідентичність):**
* Жодного `init`-коду до `main`, окрім необхідного: старт нативної програми
  дешевший за musl-static (міряється: розмір hello, RSS, час `exec`).
* Алокатор малий і поверта пам'ять ядру (`madvise`-еквівалент нативно).
* Нативні розширення як першокласні: `hx_port_*`, `hx_service_*`, `hx_shm_*`,
  `hx_display_*`, `hx_audio_*` -- не через `ioctl`.
* POSIX там, де це просто і корисно (`open/read/write/stat/pthread/socket`),
  але без історичного баласту (`gets`, `wordexp`, locale-монстр, `nis`).
* `#![no_std]` + `core`/`alloc` усередині; unsafe локалізований у `sys`.

**Міграція без "великого вибуху":**
1. `hamix_std` -> розділити на `hxlibc-sys` + `hxstd` (facade зі старим API).
2. Кожен `hx*`-крейт лишається на `hxstd`; замість `hamix_std::sys` -- виклики hxlibc.
3. C-програми: з'являється `hx-cc`; портуємо перші (`hello`, `zlib`, `lua`, `sqlite`,
   `busybox`-підмножина, `mini_png`) -- це і є "тест повноти" hxlibc.
4. `linux_bridge.rs` (278 рядків) **видаляється** з нативного коду: він
   тягнув Linux у нативне (порушення правила листа).

Критерії: усі нативні `hx*`-програми на hxlibc; `hello` C <=X КБ і старт
швидший за musl-static; тестовий набір (підмножина `libc-test`/власні) зелений;
дрібні C-бібліотеки збираються без патчів або з мінімальними.

Ризики: (1) обсяг -- вирізати за принципом "що потрібно реальним програмам"
(після W9 -- стоп нових API до 2027); (2) Rust `std` для нової ОС вимагає
форка і nightly -- лишити як [SHOULD]; запасний варіант -- нативний Rust на
`hxstd` (no_std, як зараз); (3) розбіжності з POSIX -- тестуємо на реальних
програмах, не на специфікації.

## 4A. Рішення: hxlibc = форк mlibc [MUST, ADR-0005B]

Цей розділ **заміщує** вибір "з нуля на Rust" з §4 і рекомендацію "форк relibc" з
обговорення. Мета користувача: одна libc для всього; портувати софт, а не тримати
дві libc. §4 лишається як опис шарів API і правил дизайну; реалізація -- mlibc.

### 4A.1 Що таке mlibc (перевірено за README проєкту)
* Повнофункціональна libc для портування на різні ОС і архітектури;
  C/C++, збірка **Meson**; архітектури x86-64, AArch64, RISC-V, IA-32, m68k, LoongArch64
  (наші три цілі є).
* Структура порту: `options/` (незалежне від ОС: POSIX, Linux-API як `epoll`,
  glibc-розширення -- вмикаються опціями), **`sysdeps/<os>/`** (залежне від ОС; рівно
  один на збірку) і `abis/` (лише константи й структури). Є **власний динамічний
  лінкер** `ld.so`.
* Заявлено "feature parity з glibc/musl": повні pthread, GNU-розширення. Проєкт
  повідомляє, що на ній працюють **Xorg, Wayland-композитори, Mesa, WebKitGTK**.
* Рекомендація проєкту: пінити релізи, а не збирати з master; зміни, що
  апстримяться, потрапляють під їх CI.

### 4A.2 Важливе уточнення про "запускати Linux-програми без musl"
mlibc дає **сумісність на рівні вихідного коду, а не двійкового ABI**. Тобто:
* вона дозволяє **зібрати** дуже багато Linux-програм (glibc/musl-подібні API,
  GNU-розширення, epoll тощо) під HamixOS -- **так**;
* вона **не запускає** вже зібрані бінарники з Alpine: вони скомпільовані під
  musl (інші символи, розкладка `FILE`/`pthread_t`/`struct stat`, TLS,
  інтерпретатор `ld-musl`) і без musl працювати не будуть. (Це моє знання про
  устрій libc; у README прямої заяви про двійкову сумісність немає.)

Отже "одна libc для всього" означає **іншу модель поширення софту**: не брати
готові Alpine-бінарники, а **збирати пакунки з вихідних кодів проти hxlibc**
(модель Managarm/Redox). Це велика і корисна різниця, тому нижче -- два режими.

### 4A.3 Два режими (можна йти від першого до другого)

| | Режим 1: hxlibc + Linux-шар (у плані B) | Режим 2: єдина libc (ціль користувача) |
|---|---|---|
| Нативні програми і власний софт | hxlibc (mlibc) | hxlibc |
| Чужий софт | готові Alpine-бінарники на musl через hxlinuxulator | **збирається з вихідних кодів проти hxlibc** |
| hxlinuxulator, musl | лишаються (шар, вимикається) | лишається лише для закритих/готових бінарників (напр. Chrome, Steam), або зникає |
| Час до "багато софту" | швидко (Alpine є) | повільно: кожен пакунок треба збирати й патчити |
| Ідентичність | висока | найвища: одна libc, один ABI, один світ |
| Ризик у 2026 | помірний | високий (обсяг портування, браузери) |

**Рішення:** іти **поетапно від режиму 1 до режиму 2**, не ставлячи на кін 2026 рік:
1. **2026:** hxlibc (mlibc) для всього нативного і для *нових* портів; Linux-шар на
   musl лишається для готових бінарників (Telegram, Chromium, Firefox з Alpine).
2. **2026-2027:** будуємо **конвеєр портів** і "драбину" пакунків, що
   переносяться з Alpine (рецепти `APKBUILD` беремо як основу) на hxlibc.
3. **2027+:** кожен пакунок, що вдалося перенести, виходить із Linux-шару
   в основну систему. Коли залишаться лише кілька закритих бінарників, шар
   стає необов'язковим додатком (це вже вимикається однією дією, §6.3).

### 4A.4 Порт mlibc: що робимо
* **`sysdeps/hamix`** -- реалізація кількох десятків базових функцій mlibc
  (`sys_open/read/write/close/seek`, `sys_anon_allocate/mmap/munmap`, `sys_futex_wait/wake`,
  `sys_clone/fork/execve/waitpid`, `sys_clock_get`, `sys_sigaction/sigprocmask`,
  сокети, `sys_stat`, `sys_getcwd`, `sys_tcgetattr`, TLS, `sys_libc_log/panic`).
  Стартова точка -- **`sysdeps/linux`** з mlibc (працює на сирих syscall): копіюємо і
  замінюємо виклики на нативні `SYS_HAMIX_*`; де нативного немає -- додаємо в
  Hamix ABI v1 (§5).
* **`abis/hamix`**: беремо розкладку структур Linux-подібною (`struct stat`,
  `sockaddr`, `dirent`, сигнали) -- це зменшує роботу в ядрі (спільні обробники
  зі шаром Linux) і полегшує порти. Номери викликів -- власні.
* **Опції збірки:** POSIX -- так; Linux-API (`epoll`, `signalfd`...) -- лише поки
  ядро це підтримує нативно; glibc-розширення -- так (дає найбільше зібраного софту).
* **`ld.so` -- динамічне лінкування вже в цьому році:** `libhxc.so` спільний між
  програмами (економія пам'яті: одна копія libc у page-cache), ліниве мапування
  (lazy `PT_LOAD`, план A §5 W3-W5 -- тепер це і задача hxlibc). Статичні
  `hx*`-програми лишаються можливими для мінімального старту.
* **Toolchain (`hx-cc`):** `clang`+`lld` з `--target=x86_64-unknown-hamix` (для
  clang достатньо власного sysroot і `crt0/crti/crtn`; форк LLVM на старті не
  потрібен). Meson cross-file `hamix-x86_64.txt`, аналоги для aarch64/riscv64.
* **Rust-програми:** `hxlibc-sys` (Rust-крейт із тими самими викликами Hamix ABI)
  лишається для `no_std` Rust; єдине джерело істини -- документ ABI v1 і
  згенерований з нього заголовок `hamix_abi.h`, щоб не роз'їхались C++ sysdeps і
  Rust. `hxstd` (facade зі старим API `hamix_std`) -- поверх `hxlibc-sys`.
  Порт Rust `std` (`*-unknown-hamix`, `libc`-крейт, розкладка з `abis/hamix`) --
  [SHOULD], оцінка обсягу після спайка; це не блокує C/C++ порти.
* **Ліцензія:** перевірити точний текст (README вказує "open-source", тип я не
  підтвердив) і додати в `docs/licenses`; лишити hxlibc під тією ж ліцензією,
  що й апстрім, щоб залінковані з нею програми не ставали GPL.

### 4A.5 Порівняння з relibc (для довідки)
| | mlibc | relibc |
|---|---|---|
| Мова | C/C++ | Rust |
| Модель порту | `sysdeps/<os>` + `abis/` (спеціально для нових ОС) | трейт `Pal`, бекенди `linux`/`redox` |
| Динамічний лінкер | є (`ld.so`) | є (`ld_so`) |
| Доказ на GUI-стеку | заявлені Xorg, Wayland, Mesa, WebKitGTK | Redox: менше GUI-софту (за моїм знанням) |
| Паритет із glibc/musl | заявлений | WIP |
| Відповідність "Rust-ідентичності" | нижча (ядро libc -- C/C++) | вища |

Вибір mlibc обґрунтований метою (максимум зібраного софту, менше роботи з
портуванням, доведені GUI-порти); "Rust-ідентичність" переносимо на
ядро, hxserver, Nook, hxsvc, hx-toolkit, `hxlibc-sys`, тулчейн. relibc лишається
запасним варіантом, якщо спайк mlibc провалиться.

### 4A.6 Спайк і критерій проходження (W1-W2, ~1-1.5 тижня)
1. Форк `managarm/mlibc` (пінований реліз), збірка Meson на хості.
2. `sysdeps/hamix` мінімум із `sysdeps/linux`; `hx-cc`; Meson cross-file.
3. Критерій: C-програма з `printf`, `malloc`, файлами, `pthread` і `fork/exec`
   працює на HamixOS у QEMU; `busybox` (`ash`) збирається й стартує; розмір і час
   старту зіставні з musl-static.
4. Якщо ні -- перерахувати оцінку і ADR: relibc (Rust) або відкат на musl без
   власного hxlibc.

### 4A.7 Драбина портів (конвеєр, W4-W12+)
Порядок складності, кожен щабель перевіряє попередній:
1. `busybox`, `zlib`, `libpng`, `freetype`, `harfbuzz`, `sqlite`, `lua`, `curl`, `openssl/libressl`;
2. `libffi`, `glib`, `dbus`, `libxkbcommon`, `pixman`, `cairo`, `pango`, `wayland`, `libinput`;
3. `mesa` (swrast, потім `virgl`), `gtk3`, `gtk4`, `qt6`-базові модулі;
4. XFCE-стек, `WebKitGTK` (браузер: Epiphany/Falkon-подібний -- вибір, що вже
   збирали інші ОС на mlibc); Firefox/Chromium -- **не ціль 2026** (лишаються в
   Linux-шарі з Alpine, доки не зберуться).
Механіка: `hx-ports` -- каталог рецептів (`APKBUILD` з Alpine aports як основа +
патчі), збірка в чистому sysroot, вихід -- `apk` у власний репозиторій `hamix`.

## 5. Hamix ABI v1 [MUST, ADR-0006B]

Зараз 48 `SYS_HAMIX_*`, без документа, без версії, перемішані з групами
(`AUTH`, `CMD_*`, `MKFS` -- частина взагалі не має бути syscall'ом). Робимо з
цього **стабільний нативний ABI**:
* **Аудит (W1-W2):** кожен виклик -- у таблиці `docs/reference/hamix-abi.md`:
  призначення, аргументи, помилки, гарантії, чи достатньо його для hxlibc.
* **Прибираємо з ядра те, що не syscall:** `CMD_*` (реєстр команд hsh),
  `MKFS` (утиліта `mkfs.hext`), `AUTH`/`SET_OWN_PASSWORD` (сервіс `hx-authd`
  через сервіс-порт) -- в userland, ABI стає меншим.
* **Додаємо те, чого бракує для повноцінної libc/апок (нативно):** file-backed `mmap`,
  `futex`-подібне очікування, poll/wait по багатьох джерелах (порти, fd, таймери,
  сигнали) в одному виклику, `memfd`-подібні анонімні об'єкти (це `SHM`),
  `dup/fcntl`-еквіваленти, `clock_*`, `getrandom`, робота з потоками.
* **Модель IPC як частина ідентичності:** порти + сервіси (`SERVICE_REGISTER/
  LOOKUP`, `MSG_*`, `MAILBOX_FD`) -- це наша відмінність від сокетів Linux;
  доводимо до якості (передача fd/shm-об'єктів, типізовані повідомлення, ліміти).
* **Версіонування:** `HAMIX_ABI_VERSION`, у кожній ELF-програмі -- `.note.hamix.abi`;
  ядро відмовляє за несумісної версії, старі виклики працюють мінімум один реліз.
* **Безпека:** узгодити з `docs/architecture/PERMISSIONS.md`; нові виклики за замовчуванням
  без "ambient authority" там, де це нічого не коштує.
* Правило: нова нативна можливість = виклик у ABI v1/v1.x + функція в hxlibc.

## 6. hxlinuxulator як ізольований шар [MUST, ADR-0007B]

### 6.1 Поточний стан

`kernel/src/syscall/linux/*` (~7,2 тис. рядків) + `task/linux_loader.rs` (138) +
`hxwayland` (1730 + xwm 1074) + `tools/linux-sysroot` + `overlay/opt/linux`.
Уже відокремлено на рівні коду (`syscall/native` vs `syscall/linux`), але
шар не має чітких меж: `hamix_std::linux_bridge`, спільні таблиці fd, шляхи
`/opt/linux`, `hxwayland` знає internals hxserver.

### 6.2 Особистості процесу (personality)

* Ядро при `execve` дивиться на ELF: **Hamix-особистість** -- є
  `.note.hamix.abi` (або `PT_INTERP=/lib/ld.hx` / статичний з hxlibc);
  **Linux-особистість** -- `PT_INTERP=/lib/ld-musl-*` або статичний musl.
  Особистість зберігається в `Task`, виклик `syscall` іде до відповідного
  диспетчера (вже так: `native::dispatch` / `linux::dispatch`).
* Один процес -- одна особистість. Обидві можуть жити в одній сесії, одному
  дереві процесів, спільній ФС, спільному сокетному просторі (`AF_UNIX`
  видно нативним сервісам через прозорий міст, де це потрібно).
* Специфічні для Linux вигляди (`/proc/<pid>/maps`, `/proc/cpuinfo` у
  Linux-форматі, `/dev/dri`, netlink, evdev, `/sys/class/*`) реєструє **сам
  шар** через інтерфейс `Personality` і вони зникають разом із ним.

### 6.3 Перемикач "однією дією" [MUST]

Три рівні, від простого до повного:

| Рівень | Дія | Ефект |
|---|---|---|
| Runtime | `hxctl linux off` (і в `hxsettings`) | заборона нових Linux-процесів, коректне завершення наявних, зупинка `hxwayland`/Xwayland, шар відмовляє в `execve`; пам'ять Linux-таблиць вивільняється |
| Boot | параметр `linux=0`, вибір у GRUB/установнику | шар не ініціалізується взагалі, Linux-пакунки не встановлюються |
| Образ | профіль образу `hamix-core` (без `layer=linux`) | у образі немає musl, Alpine-репозиторіїв, hxwayland, Xwayland, `/etc/init.d` |

**Ледаче завантаження шару (економія пам'яті):** Linux-особистість
ініціалізується при першому `execve` Linux-ELF, а не на старті; у профілі
"Nook без Linux-програм" вона пам'яті не займає. Спочатку -- прапорець
`LINUX_ENABLED` + ліниві структури (етап 1, W3-W6). Пізніше (2027) --
винесення шару в **завантажуваний KPI-модуль** (`personality` як новий клас
модуля), щоб його можна було й вивантажити з пам'яті (етап 2).

Умова "нічого не ламається" гарантується не обіцянкою, а інваріантами §3:
правило листа + образ `linux=off` у CI.

### 6.4 Об'єднане оточення (не `/opt/linux`)

> Оновлено 2026-09-28 (ADR-0013B): замість спільного `/usr` софт Alpine живе в окремому
> корені `/linux`, який Linux-процеси бачать як `/` (спільні `/home`, `/tmp`, `/run`, ...,
> нативне дерево -- `/hamix`). Решта пунктів нижче -- історія рішення.

* Musl (`ld-musl`, `libc.musl`) лежить у стандартних `/lib`, `/usr/lib` і належить
  пакунку `musl` (layer=linux); hxlibc -- у `/lib`, `/usr/lib` пакунка `hxlibc`
  (layer=native). SONAME різні, конфліктів немає.
* Alpine-пакунки розпаковуються прямо в `/` (як Alpine), а не в `/opt/linux`.
  `pantry` знає layer пакунка; `pantry layer linux` показує, що входить у шар;
  `pantry remove-layer linux` прибирає його (з підтвердженням).
* Один `PATH`, один `/etc`, один `/home`, одне дерево процесів; `hxfiles`
  показує все однаково; програми Linux запускаються з меню Nook, як нативні.
* `linux-sysroot` і `overlay/opt/linux` зникають; Linux-шар пакується як
  звичайні пакунки `linux-base` (musl, busybox-для-скриптів, ld-musl, alpine-keys).

### 6.5 Пакунки і репозиторії

* Репозиторії: `hamix` (native) і `alpine` (linux) -- уже підтримано
  форматом `/etc/pantry/repositories`. Кожен рядок помічений шаром.
* Власні пакунки HamixOS: формат `apk` + `APKBUILD` (одна збірка для обох) --
  той самий формат, але **власний репозиторій і власний ключ**; Alpine лишається
  джерелом чужого софту.
* Політика: пакунок `native` може залежати лише від `native`; пакунок `linux`
  -- від `linux` і `native`. Перевіряє `pantry lint` у CI.

### 6.6 Сумісність із init-системами та D-Bus-сервісами

Alpine-софт очікує OpenRC (скрипти в `/etc/init.d`) і, для DE, `login1`/udev/
polkit/UPower/UDisks2 (Alpine їх будує з elogind/eudev, **не з systemd**; тому
емуляція systemd не потрібна). Усе це -- частина шару Linux, а не системи:
* `hxsvc-openrc` -- адаптер: `hxsvc` вміє запускати скрипти OpenRC
  (`rc-service`, `openrc-run` з пакунка `openrc`, layer=linux) для Alpine-демонів.
* `hx-login1` -- D-Bus-шим `org.freedesktop.login1`, що **делегує нативним
  сервісам** `hx-session`/`hx-power`/`hx-seat`; аналогічно `UPower`,
  `UDisks2`, `polkit`-шим, `timedate1/hostname1`. Коли шар вимкнено -- шимів
  немає, нативні сервіси працюють як працювали.
* `libudev` -- `libudev-zero` + `hx-uevent` (layer=linux), нативно є свій
  `hx-devices` (події пристроїв через порти).
* `sd_notify`/`libsystemd` -- не підтримуємо (Alpine не використовує).

### 6.7 Графіка Linux-програм

* Wayland-клієнти -> `hxwayland` (шар Linux) -> **hxproto** -> hxserver. Так
  було й досі; Linux-вікна поводяться як нативні, іконки/`app_id` через
  `SetAppId`.
* X11 -> Xwayland + XWM (як є).
* **Mesa/DRM.** `/dev/dri/*`-UAPI -- частина шару Linux (`hx-drm-compat`), яка
  транслює `DRM_IOCTL_*` у **нативний GPU-клас KPI** (`DisplayOps` + нова
  3D-можливість). Так нативний клас лишається головним, а Mesa -- гість.
  Порядок (як у плані A §6.5): swrast -> `virgl` у QEMU -> Intel (розвідка).
* Що потрібно від hxwayland для Qt/Chromium/GTK (повний xdg-shell, seat,
  dmabuf, layer-shell тощо) -- W2-W7 у календарі, це прямо блокує Linux-програми.

## 7. hxserver і Nook: розвиток, а не заміна [MUST, ADR-0008B]

* **hxproto v2** (замість переходу на Wayland): нові ролі та події --
  `layer surface` (anchor, exclusive zone, layer) для панелей/доків/шпалер,
  `foreign toplevel` (список вікон для панелі/дока), workspaces, drag-and-drop,
  clipboard з типами, idle/lock, вивід (output) з масштабом, damage-tracking,
  fractional scale, `presentation feedback`.
* Це дає те саме, що дало б Wayland-нативне Nook, але **на нашому протоколі**;
  hxwayland мапить Wayland-об'єкти на hxproto 1:1 (layer-shell -> layer surface).
* `hx-toolkit` -- нативні віджети для програм Nook над hxclient/hxlibc.
* **Вигляд і анімації Nook -- без змін** (`anim.rs`, `wm.rs`, `render.rs`).
  Золотий еталон (скриншоти, покадрові послідовності, таблиця параметрів) --
  W1, і регресійний CI (як §6.3 плану A).
* Продуктивність hxserver: damage, GPU-бекенд на нативному GPU-класі
  (опційно), direct scanout/оверлеї для відео.
* Відкритий рівень: щоб сторонні програми (не Wayland) могли говорити hxproto,
  hxclient/hx-toolkit -- у hxlibc-екосистемі; є зв'язок і з C (`libhxclient.h`).

## 8. XFCE як друга DE [8.1 SHOULD -- реліз; 8.2 COULD -- експеримент]

### 8.1 Основний шлях: XFCE з Alpine, у шарі Linux (ціль релізу 0.7)

XFCE -- Linux-софт, тому живе в шарі Linux (вимкнули шар -- зникла XFCE, Nook
лишився). До кінця року очікується XFCE 4.22 з Wayland; розраховуємо на нього,
запасний варіант -- пінований dev-снепшот 4.21.x як "preview".
* **Механіка:** XFCE-сесія = `hxserver` у режимі "shell: xfce": Nook-панель/док
  вимкнено, `xfce4-panel`/`xfdesktop`/`xfce4-appfinder` -- Wayland-клієнти
  через `hxwayland`, які стають **layer surface** у hxproto v2. Вікнами керує
  hxserver (в XFCE-Wayland нема власного WM, тож це природно).
* Потрібно від hxwayland: layer-shell, foreign-toplevel, output-management,
  workspace, session-lock, idle-notify, screencopy, xdg-activation (перелік
  звіряємо з `libxfce4windowing` при старті робіт).
* Екран входу з вибором сесії (Nook / XFCE) -- нативний `hx-greeter`.
* Бюджет: XFCE-профіль <=200 МБ (ціль), міряється `membench`.
* Нативні Nook-програми працюють у XFCE-сесії, і навпаки.
* Це варіант, що реально встигає до релізу: Alpine вже зібрала GTK3, dbus,
  libxfce4* під musl -- залишається інтеграція, не портування.

### 8.2 Експеримент наприкінці плану: зібрати XFCE 4.22 нативно на hxlibc [COULD]

Мета: не "поставити ще одну DE", а **пропалити конвеєр `hx-ports` (§4A.7,
режим 2) на найбільшому реальному стеці, який у нас є на кінець року**, поки
є час до релізу, і чесно зафіксувати, наскільки далеко зайшла ідея "одна
libc для всього". Робиться в **самому кінці**, після заморожування функцій
(після W12), і **не блокує** реліз 0.7 незалежно від результату.

* **Передумова:** до W12 щаблі 1-3 драбини портів (§4A.7) уже пройдені --
  glib, dbus, libxkbcommon, pixman, cairo, pango, wayland, libinput, GTK3
  зібрані на hxlibc хоч у якомусь обсязі. Без цього спроба XFCE безглузда.
* **Обсяг спроби:** `APKBUILD`-рецепти Alpine для `xfce4-panel`, `xfdesktop`,
  `xfconf`, `garcon`, `exo`, `libxfce4util`, `libxfce4ui`, `libxfce4windowing`
  беремо як основу патчів, збираємо Meson/Autotools cross-збіркою під
  `hx-cc` + hxlibc sysroot замість musl sysroot.
* **Мінімальний прохідний критерій ("зелений")**, оскільки повна сесія
  нереалістична за такий час: **хоча б один процес XFCE (`xfce4-panel` або
  `xfdesktop`) компілюється проти hxlibc, лінкується, стартує в HamixOS і
  відкриває Wayland-з'єднання** (не обов'язково малює щось на екрані як
  повноцінна панель).
* **Проміжні щаблі, кожен -- окрема цінність, навіть якщо далі не дійде:**
  1. `xfconf` (найменша залежність, тестує dbus+glib на hxlibc);
  2. `libxfce4util`/`libxfce4ui` (тестує GTK3-залежність цілком);
  3. `xfce4-panel` або `xfdesktop` запускається і не падає одразу.
* **Що записуємо незалежно від результату:** список того, що зламалось
  (яка функція mlibc відсутня/бракує, яка бібліотека вимагає лінукс-специфіки
  на кшталт `/proc`, `epoll`, cgroup), скільки пакунків реально зібралося --
  це стає **вхідними даними для 2027 року** (справжнє портування XFCE на
  hxlibc, режим 2), а не для 0.7.
* **Порядок дій:** W12 старт, W13 спроба збірки й фіксація блокерів, W14 лише
  документація результату (`docs/history/xfce-on-hxlibc-2026.md`) -- без
  подальшого дороблення в грудні.
* Якщо W12-W13 у треку A/E і так відстають (drift) -- цей експеримент
  скасовується без жалю, це найнижчий пріоритет усього плану (див. §12).

## 9. Init і сервіси [MUST]

* **`hxinit` лишається** (ядерний boot: memory, pci, storage, rootfs, devices).
  Наприкінці -- `execve` користувацького `hxsvc` як PID 1 (деталь: pid 1 має
  бути в користувацькому процесі; перенумерація потоків ядра, W3-W4).
* **`hxsvc`** (новий, Rust на hxlibc) -- власний супервізор: декларативні
  сервіси (залежності, паралельний старт, перезапуск, ліміти пам'яті, ледачий
  старт за портом/сервісом), журнал `hxlog`, CLI `hxctl`. Формат простий,
  без роздуття systemd.
* Адаптери Linux-шару (§6.6) підключаються через `hxsvc-openrc`.
* Сесія: `hxsvc` -> `hx-session` -> hxserver -> Nook/XFCE-shell.

## 10. Пам'ять і швидкість (спільні з планом A)

Ці частини **не залежать** від вибору стратегії і переносяться з плану A
без змін (посилання на розділи `docs/roadmap/STRATEGY_2026.md`):
* §3 бюджет пам'яті і прийоми; §4 `membench` [MUST, W1];
* §6.6 швидкість ядра: `kbench`, `hxprof`, lockdep-lite, стрес `-smp 4`;
  швидкі виграші, прибрати BKL, runqueue на CPU, VFS без глобального замка,
  єдиний page-cache, NVMe/virtio-blk, tickless, швидкий boot; цілі
  відносно Alpine у QEMU;
* §4 `membench`+§6.6 `kbench`; правила CI, релізи, `CHANGELOG.md`, ADR-формат --
  ідентичні. Деталізація структури репозиторію для плану B -- окремо, §10.1
  (не просто "як у плані A", бо hxlibc і hx-ports міняють дерево суттєво).

Додатково для плану B: **бюджет шару** записується окремо -- "Linux-шар
завантажений і 0 Linux-програм": <=+4 МБ; з `hxwayland` -- <=+10 МБ.

### 10.1 Структура репозиторію для плану B [MUST, W1-W3]

Той самий діагноз, що й у плані A (`docs/roadmap/STRATEGY_2026.md` §5.0): корінь
змішує код і 500+ МБ артефактів (`hamix_disk.img`, `hamix_os.iso`, три
`rootfs*/`, `isoroot/`, `overlay/`), `overlay/opt/linux/` -- одна безхазяйна
купа, `build.sh` -- 400 рядків shell, `Cargo.lock` не в git, `docs/` без
індексу. Для плану B він гостріший: додається **новий великий підпроєкт
(hxlibc, форк mlibc, не Rust)** і **конвеєр портів (hx-ports)** -- якщо їх
кинути в корінь як є, безлад тільки зросте. Прибирання і перенесення --
одним `git mv` у W1-W2, до того як гілки hxlibc/ABI/Linux-шару розійдуться.

```
HamixOS/
  kernel/              ядро; syscall/native + syscall/linux (§6, лишається в ядрі:
                       особистість і диспетчер -- частина бінарника ядра)
  modules/             було drivers/: KPI-модулі (virtio-gpu, xhci, ahci, hx-drm-compat, ...)
  sdk/                 hamix_kpi, hamix_linuxkpi (ABI модулів ядра -- не чіпаємо)
  hxlibc/              ФОРК mlibc, окремий підпроєкт (Meson, C/C++, не Cargo):
    sysdeps/hamix/     наш бекенд (§4A.4) -- усе "своє" тут
    abis/hamix/
    options/           успадковано з апстріму, чіпаємо рідко
    vendor/            піноване upstream-дерево mlibc як git submodule/subtree,
                       щоб diff з апстрімом завжди був видимий окремо від sysdeps/hamix
  libs/                Rust-бібліотеки userland:
    hxlibc-sys/        сирі виклики Hamix ABI для Rust (§4A.4)
    hxstd/             facade зі старим API hamix_std, поверх hxlibc-sys
    hext, hxvt, hx-toolkit, кодеки (hx264, hxaac, ...)
  userland/
    core/              hsh, pantry, hxmon, hed
    services/          hxsvc (§9), hxnet, hxsound, hx-portal, hx-session
    desktop/           hxserver, Nook (panel/dock/launcher/desktop), hx-greeter, hxsettings
    apps/              hxfiles, hxterm, hxnotes, hxcalc, hxview, hxvideo, hxinstall
  layers/linux/        УВЕСЬ Linux-шар (§6), вимикається/видаляється одним каталогом:
    hxwayland/         Wayland/XWM міст (userland-частина шару)
    compat/            hx-login1, hx-polkit-shim, hxsvc-openrc, libudev-zero-конфіг
    ports/             tools/linux-sysroot нащадок: збірка musl-сирооточення для
                       готових Alpine-бінарників (режим 1, §4A.3)
  hx-ports/            конвеєр §4A.7 -- рецепти й патчі для збірки Linux-софту
                       ПРОТИ hxlibc (режим 2): `<name>/{APKBUILD-based-recipe,
                       patches/}`; вихід -- `apk` у власний репозиторій
  packages/            наші власні `apk`-пакунки (nativehx*, nook-*): `<name>/APKBUILD`
  data/                брендинг, Nook Icons, шпалери, шаблони .desktop, ліцензії
  tools/               xtask, mkhext, membench, kbench, hxprof, nook-assets
  tests/               qemu-сценарії, wayland-тест-клієнти, скриншот-еталони,
                       образ `hamix-core` (linux=off) для CI (§6.3)
  docs/                індекс + adr/ + architecture/ + guides/ + reference/ + history/
  out/                 (gitignored) усе згенероване: образи, iso, rootfs*, isoroot, логи
```

Ключові рішення структури, специфічні для плану B:
* **`hxlibc/` -- окремий верхній каталог, не `libs/`.** Це чужий апстрім
  (Meson, C/C++, власна історія), а не наш Rust-крейт; тримати його поруч із
  `kernel/` явно каже "це другий великий підпроєкт", а не бібліотека серед
  бібліотек. `vendor/` як git subtree з піно­ваним тегом -- оновлення mlibc
  це один PR, що показує diff, а не переписування sysdeps/hamix.
* **`layers/linux/` -- усе, що вимикається §6.3, в одному місці.** Правило
  листа (§3, п.1) перевіряється буквально: жоден файл поза `layers/linux/` не
  може імпортувати/лінкувати щось із нього (лінтер `tools/xtask lint-layers`
  у CI: `grep`/аналіз залежностей Cargo.toml + Meson-залежностей).
* **`hx-ports/` і `packages/` розділені навмисно:** `packages/` -- те, що ми
  публікуємо як готові `apk` (і наше нативне, і перезібране з Alpine), а
  `hx-ports/` -- "верстат", рецепти й патчі, що ще не завжди збираються
  (щаблі драбини §4A.7 явно можуть бути "червоними" без шкоди для релізу).
* `kernel/src/syscall/linux/` **лишається в ядрі**, не в `layers/linux/` --
  бо це частина бінарника ядра, а не userland-пакунок; межа шару описана
  інтерфейсом `Personality` (§6.2), не директорією.

Правила (спільні з планом A, `CONTRIBUTING.md`):
1. Усе згенероване -- лише в `out/`, у корені жодних образів/rootfs.
2. Одна точка входу: **`cargo xtask`** (`build/run/test/iso/membench/kbench/
   lint-layers/release`), з `--arch`; для `hxlibc/` -- `xtask hxlibc-build`
   обгортає Meson, щоб не було двох незалежних інструкцій збірки для людини.
3. `Cargo.lock` у git; `hxlibc/vendor` пінований тегом; `hx-ports/*` -- пінований
   коміт джерела в кожному рецепті.
4. Кожен файл в образі має власника-пакунок (`pantry files/owner`), позначеного
   `layer = native | linux` (§6.5) -- лінтер ловить порушення правила листа.
5. Іменування: `hx*` -- нативні системні компоненти, `nook-*` -- частини Nook,
   апстрімні назви -- як є (`xfce4-panel`, не `hx-xfce-panel`).
6. `rustfmt`+`clippy` для Rust, `clang-format` для `hxlibc/sysdeps/hamix`,
   збірка трьох архітектур, boot-тест і **образ `hamix-core` без Linux-шару**
   (§6.3) -- усе обов'язкове в CI до merge.

Документація -- та сама структура, що в плані A §5.0 (`docs/{README,
architecture,guides,reference,adr,history,licenses}`), плюс `docs/reference/
hxlibc-status.md` (генерується з `hx-ports/`: що зібралося на hxlibc, що ні --
живий "application compatibility list" для режиму 2) і ADR-документи 0005B-0011B
цього плану (одне рішення -- один файл в `docs/adr/`).

Порядок у самій системі (образ), відмінності від плану A:
* без `/opt/linux`, розклад -- Alpine FHS (§6.4), Linux- і нативні пакунки в
  тих самих `/usr`, `/etc`, різні лише SONAME (`libhxc.so` vs `libc.musl-*.so`);
* `/etc/hamix` -- нативні налаштування; `/etc/init.d`+`/etc/conf.d` --
  лише для Alpine-демонів шару (§6.6), нативні сервіси -- через `hxsvc` (§9),
  не OpenRC-скрипти (це відмінність від плану A, де OpenRC -- єдиний менеджер);
* один журнал `hxlog` для нативних сервісів; сервіси шару логуються туди ж
  через адаптер `hxsvc-openrc`;
* `/usr/share/hamix`, шляхи Nook -- через `hamix_paths` (той самий принцип).

Вихід W3: дерево перенесене, `cargo xtask build/run/test` працює на трьох
архітектурах, `hxlibc/` збирається окремим кроком, `lint-layers` зелений,
docs мають індекс і перші ADR.

## 11. Календар за треками

Треки йдуть паралельно; тижні: W1=24.09 ... W14=31.12.

**Трек A: hxlibc = mlibc (§4A)**
* W1-W2: ADR; спайк порту (§4A.6): форк mlibc, `sysdeps/hamix` мінімум, `hx-cc`, C hello; **рішення go/no-go в кінці W2**.
* W2-W6: повний `sysdeps/hamix` (файли, mmap, потоки/futex, fork/exec, сигнали, сокети, tty), `abis/hamix`, тести.
* W4-W8: `hx-cc` + Meson cross-file для трьох архітектур; `ld.so` і `libhxc.so`; `hxlibc-sys` + `hxstd`, міграція 19 крейтів, видалення `linux_bridge`.
* W6-W12: конвеєр портів `hx-ports`: щаблі 1-3 драбини (§4A.7); [SHOULD] щабель 4 (WebKitGTK).
* W9-W12: порівняння з musl (розмір, старт, `malloc`, `memcpy`, `printf`); [SHOULD] Rust `std` після оцінки.
* Вихід: усі нативні програми на hxlibc, динамічна libc працює, >=30 пакунків зібрано з вихідних кодів на hxlibc.

**Трек B: Hamix ABI v1**
* W1-W2: аудит 48 викликів, `docs/reference/hamix-abi.md`.
* W2-W5: винесення `CMD_*/MKFS/AUTH` з ядра; версіонування, ELF-нота.
* W3-W8: нові нативні виклики (file mmap, futex, універсальний wait, memfd/shm).
* W8-W12: заморожування ABI v1, тести на кожен виклик.

**Трек C: ізоляція шару Linux**
* W1-W2: інвентар залежностей нативного від Linux; правило листа; мітки шару в pantry.
* W2-W4: інтерфейс `Personality` у ядрі; ELF-нота/інтерпретатор -> вибір особистості.
* W3-W6: `LINUX_ENABLED`, ліниве ініціалізування; `hxctl linux on|off|status`.
* W4-W7: прибрати `/opt/linux`; `linux-base` пакунки; `pantry` шари; `hxwayland`/
  Xwayland -> пакунки шару.
* W5-W8: образ `hamix-core` (без Linux) у CI + весь нативний набір тестів.
* W8-W12: установник із вибором "Linux-сумісність"; `hxsettings` перемикач.
* [COULD] W12+: шар як завантажуваний модуль.

**Трек D: hxserver/Nook**
* W1: золотий еталон Nook.
* W3-W8: hxproto v2 (layer surface, foreign toplevel, workspaces, dnd, clipboard, output/scale).
* W4-W10: `hx-toolkit`, міграція нативних програм на v2.
* W6-W12: продуктивність, direct scanout, опційний GPU-бекенд.

**Трек E: сумісність Linux-програм**
* W2-W7: hxwayland: повний `xdg-shell`, `wl_seat`, `data-device`, `viewporter`,
  `fractional-scale`, `xdg-decoration`, неблокуюча відправка, `linux-dmabuf`.
* W3-W6: прогалини ядра для Linux ABI (lazy `PT_LOAD`, `memfd`, inotify,
  `/proc` для GTK/Qt/Chromium, Chromium #GP).
* W6-W10: Chromium/Firefox, GTK4/Qt6 hello, Telegram стабільно.
* W6-W12: XFCE (§8) -- пакунки, layer-shell, сесія, preview.

**Трек F: init/сервіси**
* W3-W4: pid 1 у користувацькому просторі; `hxsvc` MVP.
* W4-W8: залежності, ледачий старт, ліміти, `hxlog`, `hxctl`.
* W6-W10: адаптери Linux-шару (`hxsvc-openrc`, `hx-login1`, `libudev-zero`, polkit/UPower/UDisks2).

**Трек G: GPU**
* W2-W4: дизайн нативного GPU-класу (3D-можливість), `/dev/dri` як шар Linux (ADR).
* W4-W7: `hx-drm-compat` + swrast Mesa, `kmscube`.
* W6-W10: `virtio-gpu` 3D (`virgl`); [SHOULD] опційний GPU-бекенд hxserver.
* W10-W12: розвідка Intel (ціль-покоління, обсяг); [COULD] Venus.

**Трек H: спільне (з плану A §10) + структура проєкту (§10.1)**
* W1-W2: `membench`, `kbench`, `hxprof`, lockdep-lite, стрес `-smp 4`.
* W1-W2: **перенесення дерева репозиторію за §10.1** (`git mv` одним PR, до
  розходження гілок hxlibc/ABI/Linux-шару) -- блокує старт трека A і C.
* W2-W3: `cargo xtask` (build/run/test/iso/hxlibc-build/lint-layers),
  `Cargo.lock` у git, docs-індекс, перші ADR (0005B-0008B), `lint-layers` у CI.
* W2-W12: швидкість ядра (за пріоритетами §6.6 плану A).
* Вихід W3: дерево з §10.1 на місці, `xtask` покриває три архітектури, кожен
  наступний трек працює вже в новій структурі.

**Трек I: експеримент "XFCE 4.22 на hxlibc" (§8.2) [COULD, останній пріоритет]**
* W12: старт лише якщо треки A/E не в дрейфі; беремо `APKBUILD` XFCE як основу патчів.
* W13: збірка `xfconf` -> `libxfce4util`/`libxfce4ui` -> `xfce4-panel`/`xfdesktop`
  проти hxlibc; фіксація кожного блокера.
* W14: без нового коду -- лише запис результату в `docs/history/xfce-on-hxlibc-2026.md`
  (що зібралося, що зламалось, оцінка обсягу на 2027), реліз 0.7 на нього не чекає.

**W12-W14 (16.12-31.12): стабілізація і випуск 0.7**
* Замороження функцій 16.12; реальне залізо; документація (`hxlibc`, ABI v1,
  шар Linux, `PORTING_GUIDE` для двох шляхів: "як портувати на hxlibc" і "як
  поставити з Alpine"); скриншоти; ISO двох профілів (`hamix-core`, `hamix-full`).

## 12. Що обов'язкове, а що ріжемо при браку часу

MUST: hxlibc (sys/core/C ABI/hx-cc), `hxstd` міграція, ABI v1 аудит і документ,
ізоляція шару Linux і `linux=off` образ у CI, hxproto v2 (мінімум для панелі),
золотий еталон Nook, hxsvc MVP, pid 1 у userland, `membench`/`kbench`, порядок у проєкті.
SHOULD: Rust `std` для Hamix, XFCE preview (з Alpine, §8.1), virgl, NVMe, розвідка Intel.
COULD: шар як модуль, Venus, huge pages, io_uring, динамічна hxlibc,
**експеримент XFCE 4.22 на hxlibc (§8.2)**.
Ріжемо в порядку: **XFCE на hxlibc (§8.2)** -> Venus/Intel -> GPU-бекенд hxserver
-> Rust std -> XFCE preview (§8.1, лишається як preview навіть скорочена).
Не ріжемо: hxlibc, ABI v1, перемикач Linux і його CI, Nook-еталон.

## 13. Ризики

| Ризик | Ймовірність | Що робимо |
|---|---|---|
| Обсяг порту mlibc і рецептів `hx-ports` | висока | Спайк із go/no-go на W2; пінований реліз mlibc; драбина портів по щаблях; ціль 2026 -- 30+ пакунків, не весь Alpine |
| Режим 2 (єдина libc) не встигає: Alpine-бінарники на musl не запускаються на mlibc | висока | Не залишаємо Linux-шар: поетапно від режиму 1 до 2 (§4A.3); Chromium/Firefox лишаються в шарі |
| Зміни апстріму mlibc ламають наш sysdeps | середня | Пінований реліз, `sysdeps/hamix` малий і ізольований, регулярне оновлення раз на реліз |
| Два ABI = вдвічі більше підтримки (як було раніше) | висока | Правило листа; спільні внутрішні примітиви (fd-таблиця, VFS, mm); Linux-шар тонкий і змінюється лише за потреби реальних програм |
| Rust `std` для Hamix вимагає форка/nightly | середня | [SHOULD]; запасний варіант -- `hxstd` (no_std), як зараз |
| Linux-шар тягне нативне назад (непомітні залежності) | середня | Образ `linux=off` у CI, `pantry lint`, заборона `linux_bridge` |
| hxproto v2 не покриває потреби XFCE | середня | Мінімальна підмножина layer-shell на старті; XFCE як preview |
| Обсяг для 14 тижнів надто великий | дуже висока | Порядок скорочення §12; тижнева п'ятниця-ретроспектива |
| Реальне залізо | висока | Реліз орієнтується на QEMU + перелік перевіреного заліза |
| Експеримент §8.2 (XFCE на hxlibc) не встигає або зриває графік треків A/E | середня | Найнижчий пріоритет у §12; старт лише за умови, що треки не в дрейфі; W14 -- тільки документація без коду; скасовується без жалю |

## 14. Порівняння з планом A і рекомендація

| | План A (Linux-first) | План B (Hamix-first) |
|---|---|---|
| Головний ABI | Linux; `SYS_HAMIX_*` -> 0-5 | Hamix ABI v1 (розвиток); Linux -- шар |
| libc | musl для всього | hxlibc (нативні) + musl (Linux) |
| Вікна | Nook -> Wayland-композитор; hxproto видаляється | hxserver/hxproto v2 лишається; Wayland тільки для Linux-програм |
| Init | OpenRC як PID 1 | hxinit + hxsvc; OpenRC лише адаптер для Alpine |
| Linux можна вимкнути | ні (це й є система) | так, однією дією; образ `linux=off` у CI |
| Ідентичність | Rust-ядро + бюджет пам'яті; решта -- Linux-екосистема | шість власних великих систем; Linux -- гість |
| Обсяг власної роботи | менший (більше готового з Alpine) | більший (hxlibc, hxsvc, ABI v1, hxproto v2) |
| Ризик недоробити | середній (багато, але відоме) | вищий (hxlibc -- нове, невідомий обсяг) |
| Сумісність з Linux-софтом | максимальна, найшвидше | така сама, але через шар; XFCE/Chromium залежать від роботи в шарі |
| Час до "працює все" | коротший | довший |

Рекомендація: **план B**, якщо ключове -- ідентичність і те, що "реально своє";
план A -- якщо ключове -- швидко отримати повноцінний десктоп із максимумом софту.
Компроміс, який я б обрав: **план B з hxlibc = mlibc (§4A)**, поетапно від режиму 1
до режиму 2, зі спайком і рішенням go/no-go на W2.

Відкриті питання до вас (оновлено після вибору mlibc):
1. **Режим 2 (єдина libc, порти з вихідників) як довгострокова ціль, режим 1
   (Linux-шар на musl) лишається на 2026** -- підходить? Це головне рішення §4A.3.
2. Чи прийнятно, що ядро libc -- C/C++ (mlibc), а "Rust-ідентичність" -- це ядро,
   hxserver/Nook, hxsvc, hx-toolkit, `hxlibc-sys`, тулчейн?
3. XFCE у шарі Linux (зникає з вимкненням) -- поки порт GTK/XFCE на hxlibc не готовий (2027).
4. Чи лишаємо в ABI v1 порти/сервіси як головну IPC-модель (моя рекомендація -- так)?
