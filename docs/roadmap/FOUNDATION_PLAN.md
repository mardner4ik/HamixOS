# План F: власна основа -- hxlibc з нуля, справжній Wayland у Nook, вхід без X11

Status: active · Started: 2026-10-07 · Updated: 2026-10-27 · Продовжує план U (`UNIX_PLAN.md`), замінює ADR-0005B
(hxlibc як форк mlibc) і частину ADR-0014B (hxwayland як міст). Порядок робіт -- §13.

Рішення, що визначають цей план:

1. **hxlibc пишеться заново** як незалежна libc (Rust + мінімум асемблера), орієнтована на
   POSIX.1-2024 і BSD. Форк mlibc видаляється, коли останній порт перейде на нову бібліотеку.
2. **Nook стає справжнім Wayland-композитором**: Wayland-поверхня -- це вікно сцени hxserver
   без перекладу в hxproto і без копіювання пікселів. Окремий процес і бібліотека-міст
   `hxwayland` зникають.
3. **LightDM і Xorg виходять із базової системи.** Вхід -- власний невеликий менеджер `hxdm` з
   Wayland-привітанням на Nook. X11 лишається лише як необов'язковий модуль (Xwayland), а
   GNU/Linux-спадщина libc -- в окремій бібліотеці `libhxcompat`.
4. **Власний завантажувач `hxboot` замість GRUB** (Rust, BIOS + UEFI, multiboot2-сумісний,
   з перевіркою підпису ядра) -- §8.
5. **Максимальний захист від дір і перехоплення даних** на рівні ядра й hxlibc: W^X і SMEP/SMAP
   для ядра, KASLR/ASLR, `pledge`/`unveil`, ізоляція вводу, екрана й пам'яті процесів -- §9.
6. **`hsh` замість bash:** повертаємось до власної оболонки на Rust, максимально сумісної з bash, і
   видаляємо bash з базової системи для легкості -- §10.
7. **Легка й швидка система:** кожен компонент базової системи має обґрунтований бюджет диска,
   пам'яті та часу запуску; зайві служби, залежності, копії даних і фонові процеси не потрапляють
   до стандартного образу. Продуктивність вимірюється на слабкому залізі й не погіршується без
   поясненої причини -- §2, §7 і етапи цього плану.
8. **Власне легке робоче середовище:** на базі `hxserver` створюється проста DE з трохи ретро
   інтерфейсом, меню програм, вікнами та базовими налаштуваннями. Власна тема HamixOS задає
   спільний вигляд системи й програм на `hxproto`/`hxtoolkit`.

---

## 1. Діагноз (стан на 2026-10-07)

### 1.1 libc
* `hxlibc/` -- mlibc 5.0.0 (managarm, MIT) + 26 наших патчів (`applied-patches/0001..0026`).
  Сама mlibc не з GNU, але несе шари сумісності `options/glibc`, `options/linux`, `options/lsb`,
  і вони, а не POSIX, визначають її форму: 1738 експортованих символів, `libhxc.so` -- 876 КіБ,
  `ld.hx` -- 136 КіБ.
* Нашим програмам потрібна менша частина: 598 ELF-файлів живого образу імпортують із libc **812**
  символів (`llvm-nm -D --undefined-only`, 2026-10-07). Серед них Linux/GNU-специфічних лише
  кілька десятків: `epoll_*`, `inotify_*`, `signalfd`, `timerfd_*`, `eventfd`, `memfd_create`,
  `prctl`, `unshare`, `statx`, `sendfile`, `splice`, `sysinfo`, `getauxval`, `secure_getenv`,
  `strchrnul`, `memrchr`, `backtrace`, `__fpurge`, `__fsetlocking`, `getutxent`, `getspnam`,
  `index`/`rindex`, `mktemp`/`tmpnam`, `getdtablesize`, `vfork`, `__progname`. Решта -- ISO C,
  POSIX і BSD (`strlcpy`, `strlcat`, `reallocarray`, `getentropy`).
* Внутрішні проблеми форку, які ми вже ловили:
  * аллокатор frigg тримав по 4 МіБ на процес (патч 0011);
  * `calloc` завжди обнуляв пам'ять;
  * printf без плаваючої коми (взяли з musl, патч 0009);
  * scanf і `setvbuf` поводилися неправильно (0019, 0020);
  * rtld без TLSDESC (0022).
  Кожен патч -- це розбіжність з апстрімом, яку доведеться переносити при оновленні.
* Мова реалізації -- C++ (frigg, cxxshim), збірка -- Meson. Безпека пам'яті та сама, що в будь-якій
  C-libc; Rust-частина системи (hxstd, Nook, hxserver) отримує ці гарантії лише до межі FFI.
* Розкладка структур і номери помилок -- як у Linux (ADR-0005B), і це правильно: ядро вже говорить
  цією мовою, а Linux-шар на musl працює поруч.

### 1.2 Wayland у Nook
* Режим «Nook (Wayland)» запускає оболонку `nook-shell` як звичайний Wayland-клієнт, але
  серверна сторона -- це **код мосту hxwayland, вбудований у hxserver** (`wayland::Embedded`):
  * кожен запит клієнта перекладається в hxproto `Request`;
  * вікно стає вікном hxproto (`Body::Client`), а пікселі буфера копіюються в окрему пам'ять
    hxproto (`alloc_buffer` + копія в `show`);
  * події вводу проходять зворотний переклад `Event` → `wl_pointer`/`wl_keyboard`;
  * XWM (`xwm.rs`, 1081 рядок) живе в тому ж мості.
* Наслідки:
  * зайва копія кожного кадру кожного вікна (для 1366×768 це 4 МіБ на кадр);
  * затримка вводу через подвійний переклад;
  * два джерела правди про вікно (стан мосту і стан hxserver);
  * обмеження hxproto стають обмеженнями Wayland (немає справжніх субповерхонь, viewporter, точних
    `frame`-колбеків).

### 1.3 Вхід у систему
* LightDM 1.32 + lightdm-gtk-greeter вимагають X-сервера для привітання. Через це в образі живуть:
  * Xorg 21.1, xf86-input-libinput;
  * libxklavier, libXfont2, libfontenc, libpciaccess, xkbcomp-ланцюжок;
  * linux-pam і GTK3-X11-бекенд.
* Ціна цього:
  * десятки мегабайт;
  * перемикання VT між Xorg і Nook (`adopt_vt`, `waitid(P_PIDFD)`, 5-секундні повтори
    `Screen::take`);
  * гонки зі стартом D-Bus.

### 1.4 Ядро -- головне джерело повільності на слабкому залізі
* Файлові `mmap` (усі `.so`) наповнюються через `lazy_fault` (`syscall/base.rs`): **кожен процес
  отримує власну копію кожної сторінки** бібліотеки.
  * Копія робиться через `read_file` під `VFS.lock()`, тобто фактично під великим замком ядра.
  * Старт GTK-програми копіює десятки мегабайт (Mesa, GTK, GLib, libc), а всі інші процеси в цей
    час чекають на замок.
  * У профілі простою в QEMU `bkl_contended` = 12 140 за 10 с.
* Немає vDSO: `clock_gettime` (а з ним анімації Nook, таймери GLib, `poll`-цикли) іде повним
  системним викликом.

---

## 2. Цілі та метрики

| Метрика (x86_64, QEMU KVM, 1 ядро, і Samsung R428) | Зараз | Ціль |
|---|---|---|
| `libc.so` (stripped) | 876 КіБ (`libhxc.so`) | ≤ 450 КіБ |
| динамічний завантажувач | 136 КіБ (`ld.hx`) | ≤ 64 КіБ |
| `libhxcompat.so` | -- | ≤ 120 КіБ, підвантажується лише за потреби |
| RSS приватна, `hello` на C | вимірюємо в F1 | ≤ 160 КіБ |
| `exec` + `exit` `hello` | вимірюємо в F1 | ≤ 150 мкс (KVM) |
| `malloc`/`free` 16-256 Б, один потік | вимірюємо в F1 | ≤ 20 нс (на рівні mimalloc) |
| `memcpy` 4 КіБ, SSE2 | вимірюємо в F1 | ≥ 90 % від glibc |
| `printf("%d %s %.3f")` | вимірюємо в F1 | ≥ musl |
| `unsafe` у реалізації libc | весь C++ | лише модулі `abi/`, `sys/`, `rt/`, `rtld/`, `alloc/raw` |
| кадр Wayland-вікна 1366×768 | копія 4 МіБ + переклад | 0 копій, композиція прямо з буфера клієнта |
| запуск сесії від введення пароля | LightDM → Xorg → Nook, 3-6 с | `hxdm` → Nook ≤ 1 с |
| сторінки `.so`, спільні між процесами | 0 % | 100 % для read-only сегментів |
| стандартні фонові служби | вимірюємо в F0 | лише необхідні для базової сесії |
| пам'ять після входу та час до готового робочого столу | вимірюємо в F0 | бюджет і контроль регресій для кожного релізу |
| базові програми з темою системи | різні інтерфейси | єдина тема `hxproto`/`hxtoolkit` |

Кожна метрика має автоматичний замір (`cargo xtask kbench`, нові `libc-bench` і `size-gate`, §7) і
ворота: регресія понад 5 % -- помилка збірки.

## 3. Що НЕ робимо

* Не ламаємо Linux-шар: `/linux` (Alpine + musl) лишається. Нова hxlibc потрібна лише нативним
  програмам.
* Не міняємо числовий ABI ядра (номери викликів, `errno`, сигнали, розкладка `stat`/`sockaddr`).
  «Ближче до BSD» -- це про API, дисципліну і стиль, а не про інші числа.
* Не пишемо свій libm з нуля: беремо Rust-порт musl libm (крейт `libm`, MIT) і доводимо до ладу.
* Не пишемо свій C++-рантайм: лишаються LLVM libc++/libc++abi/libunwind (`ports/libs/libcxx`).
* Не видаляємо hxproto: нативні програми Nook і далі говорять hxproto. Wayland стає другим
  рівноправним фронтендом.
* Не видаляємо mlibc одним комітом: міграція йде по одній програмі, обидві libc живуть поруч, доки
  є хоч один споживач старої (§4.12).

---

## 4. Частина A: hxlibc -- нова libc

### 4.1 Принципи
1. **Мала:** у ядрі тільки ISO C17/C23, POSIX.1-2024 (Base + XSI, де його реально використовують) і
   перевірені BSD-розширення. Усе інше -- у `libhxcompat` (§4.10).
2. **Безпечна:**
   * логіка (форматування, розбір, пошук, локалі, DNS) -- безпечний Rust;
   * `unsafe` лише на межі C ABI і в системному шарі;
   * захист від експлуатації ввімкнено за замовчуванням (§4.7).
3. **Швидка на слабкому залізі:**
   * базова лінія -- x86_64 без AVX (Core 2 / Pentium T-серії у Samsung R428);
   * SSE2 завжди, швидші шляхи -- за `cpuid` при старті;
   * однопотокові програми не платять за блокування.
4. **Передбачувана:** одна реалізація кожної функції, жодних «stubs», що мовчки повертають
   `ENOSYS`. Відсутня функція -- це помилка лінкування, а не сюрприз під час роботи.
5. **Тестована на хості:** уся бібліотека, крім `rt`/`rtld`, збирається і тестується на Linux-хості
   (бекенд `sys::linux`) з порівнянням поведінки з musl.

### 4.2 Шари

```
 програми C/C++ (порти)          Rust-програми (hxstd)
         │  C ABI                        │  C ABI (а згодом -- безпечний hxc-API напряму)
 ┌───────▼───────────────────────────────▼──────────────────────────────┐
 │ abi/      extern "C" функції: перевірка вказівників, NUL-рядків,     │  unsafe
 │           перетворення в безпечні типи; errno; символи-аліаси        │
 ├──────────────────────────────────────────────────────────────────────┤
 │ core/     безпечна логіка: fmt (printf/scanf), num (strtod, Ryu),    │  #![forbid(unsafe_code)]
 │           string/ctype/wchar, locale, time (tz, strftime), regex,    │
 │           search, resolver (DNS/hosts), passwd/group, crypt, getopt  │
 ├──────────────────────────────────────────────────────────────────────┤
 │ io/       stdio (FILE, буфери, блокування), dirent, temp files       │  мінімум unsafe
 │ thread/   pthread, futex-примітиви, TLS, once, keys, cancellation    │
 │ alloc/    malloc (hxmalloc): політика (safe) + сирі сторінки (unsafe)│
 │ mem/      memcpy/memset/memcmp/strlen: SSE2 + вибір за cpuid         │  asm/intrinsics
 ├──────────────────────────────────────────────────────────────────────┤
 │ sys/      системні виклики, типізовані: Fd, Pid, Errno, Result<T>    │  unsafe (asm)
 │           бекенди: hamix (x86_64/aarch64/riscv64), linux (хост-тести)│
 ├──────────────────────────────────────────────────────────────────────┤
 │ rt/       crt1/crti/crtn, _start, auxv, AT_RANDOM, TLS-блок,         │  asm + unsafe
 │           __libc_start_main, atexit, init/fini, stack protector      │
 │ rtld/     ld-hamix.so.1: ELF, GNU hash, RELR, TLS, TLSDESC, dlopen   │
 └──────────────────────────────────────────────────────────────────────┘
```

Правило залежностей (перевіряється `cargo xtask lint-layers`):
* `core` не бачить `sys`, `abi` і `alloc::raw`;
* `abi` -- єдиний модуль, що торкається сирих вказівників користувача;
* `compat` залежить лише від публічного C API ядра бібліотеки.

### 4.3 Розкладка репозиторію

```
hxlibc/
  Cargo.toml              workspace бібліотеки (окремий від кореня: власна ціль, panic=abort)
  crates/
    hxc-sys/              системні виклики й типи ядра (no_std, без alloc)
    hxc-core/             безпечна логіка (no_std + alloc, тестується на хості звичайним cargo test)
    hxc-alloc/            hxmalloc
    hxc-mem/              memcpy & co, вибір реалізації
    hxc-io/  hxc-thread/  stdio і потоки
    hxc-rt/               старт процесу, TLS
    hxc-rtld/             динамічний завантажувач
    hxc/                  cdylib + staticlib: abi/ (extern "C"), збирає libc.so.1 і libc.a
    hxc-compat/           cdylib libhxcompat.so.1 (§4.10)
    hxc-libm/             libm (порт musl libm) -- у libc.so, libm.so -- порожня заглушка
  include/                заголовки, написані вручну (§4.5)
  arch/{x86_64,aarch64,riscv64}/   crt*.S, setjmp.S, sigreturn, clone, syscall-шаблони
  tests/
    unit/                 cargo test на хості
    abi-check/            генератор static_assert(sizeof/offsetof) з Rust-типів → clang
    conformance/          libc-test (musl, MIT), open_posix_testsuite (лише як зовнішній раннер)
    diff/                 диференційні тести і фазинг проти musl на хості (cargo fuzz)
    bench/                libc-bench
  legacy-mlibc/           старий форк на час міграції; видаляється в F7
```

Збірка -- `cargo xtask hxlibc-build`. Вона збирає Rust-частину для цілей `x86_64-hamix-libc` (без
libc, `-C relocation-model=pic`, `panic=abort`, `opt-level=3`, LTO, `codegen-units=1`) і асемблер
через clang, кладе sysroot у `out/hxlibc/ARCH/sysroot`. Meson і C++ зникають з ланцюжка.

### 4.4 Рішення щодо ABI
* **Числа ядра -- Linux** (як зараз): номери викликів основного діапазону, `errno`, сигнали,
  `struct stat`, `sockaddr`, `termios`, `ioctl`. Нативне ядро, Linux-шар і `hxlibc` говорять однією
  мовою; жодного перекладу в ядрі.
* **Типи без компромісів 1990-х:**
  * `time_t`, `off_t`, `ino_t`, `blkcnt_t` -- 64-бітні на всіх архітектурах;
  * жодних дублікатів `*64` (`stat64`, `lseek64`, `off64_t`). Для старого коду вони є лише як
    макроси в `<compat/lfs64.h>`.
* **Імена файлів:**
  * бібліотека -- `/lib/libc.so.1` (SONAME `libc.so.1`), завантажувач -- `/lib/ld-hamix.so.1`;
  * `libm`, `libpthread`, `librt`, `libdl`, `libutil`, `libcrypt` -- порожні `.so` або лінкер-скрипти
    для сумісності з `-lm`/`-lpthread`. Усе живе в `libc.so.1`.
* **Без версіонування символів** (`GLIBC_2.x`): ABI змінюємо лише разом зі SONAME. Це простіше,
  менше і швидше в rtld.
* **Перехідний період:** старі програми мають `PT_INTERP=/lib/ld.hx`, нові --
  `/lib/ld-hamix.so.1`. Ядро нічого не знає про різницю, а дві libc ніколи не потрапляють в один
  процес.

### 4.5 Заголовки
* Пишуться вручну, по одному на стандартний заголовок, у стилі BSD:
  * читабельні, без генераторів;
  * видимість через `<sys/cdefs.h>`-макроси `__POSIX_VISIBLE`, `__XSI_VISIBLE`, `__BSD_VISIBLE`,
    `__ISO_C_VISIBLE` (як у FreeBSD).
* Макроси вибору функціональності:
  * `_POSIX_C_SOURCE`, `_XOPEN_SOURCE`, `_DEFAULT_SOURCE` (= POSIX.1-2024 + BSD) -- ядро бібліотеки;
  * `_GNU_SOURCE` вмикає `__BSD_VISIBLE` і додатково підключає оголошення з
    `<compat/*.h>` (`libhxcompat`).
* Безпечні атрибути в заголовках:
  * `__attribute__((nonnull))`, `format(printf)`, `warn_unused_result` для `realloc`/`read`;
  * `__attribute__((deprecated("use posix_spawn")))` для `vfork`, `tmpnam`, `mktemp`;
  * `gets` відсутня взагалі.
* **Перевірка ABI** (`tests/abi-check`): з Rust-визначень `#[repr(C)]` генерується C-файл із
  `_Static_assert(sizeof/offsetof/alignof)` для кожної структури й константи; clang компілює його з
  нашими заголовками для кожної архітектури. Розбіжність -- помилка збірки.
  (relibc генерує заголовки cbindgen-ом; ми навпаки: заголовки -- вручну, перевірка -- автоматична.)

### 4.6 Модулі

**sys** -- типізовані виклики:
* `fn read(fd: Fd, buf: &mut [u8]) -> Result<usize, Errno>`, `fn openat(dir: Fd, path: &CStr, ...)`;
* syscall-шаблони -- `asm!` для кожної архітектури;
* бекенд `linux` (той самий набір через Linux-ядро хоста) дає змогу запускати весь `cargo test`
  бібліотеки без QEMU.

**rt** -- старт процесу:
* `_start` → `__hxc_start(sp)`: розбір `argv/envp/auxv`;
* TLS: статичний блок і TCB з канарейкою на `%fs:0x28`;
* `AT_RANDOM` → канарейка стека, ключ манглування вказівників і насіння `arc4random`;
* `AT_HWCAP`/`cpuid` → вибір `mem`-реалізацій;
* `init_array`/`fini_array`, `atexit`, `__cxa_atexit`, `__cxa_finalize`, `__stack_chk_fail`;
* статичні PIE підтримуються (`libc.a` + `rcrt1.o`) -- для `hxsvc`, `hx-authd`, аварійних утиліт.

**rtld** (`ld-hamix.so.1`):
* формат: ELF64, `DT_GNU_HASH` (пошук за bloom-фільтром), `DT_RELR` (компактні релокації),
  `PT_GNU_RELRO`;
* `BIND_NOW` завжди (повний RELRO, без лінивого зв'язування);
* TLS: static + dynamic, `__tls_get_addr`, TLSDESC для x86_64/aarch64/riscv64;
* API: `dlopen`/`dlsym`/`dlclose`/`dladdr`/`dl_iterate_phdr` (останнє потрібне libunwind);
* `$ORIGIN`, `LD_LIBRARY_PATH` (ігнорується для setuid), кеш шляхів `/etc/ld-hamix.cache`
  (бінарний, генерує `ldconfig`).
* Цілі розміру й швидкості:
  * без alloc-купи до завершення релокацій;
  * завантажувач релокує себе сам;
  * `mmap` сегментів з правильними правами одразу (не `mprotect` по сторінці);
  * спільні сторінки -- через спільний кеш сторінок ядра (§6, F0).

**alloc** -- hxmalloc:
* Дизайн за мотивами mimalloc і OpenBSD malloc:
  * сторінки по 64 КіБ, класи розмірів 16 Б-32 КіБ, по одному вільному списку на сторінку
    («free list sharding»);
  * кеш на потік без блокування;
  * звільнення з чужого потоку -- атомарний список на сторінці.
* Великі блоки (≥ 128 КіБ) -- напряму `mmap` з охоронними сторінками.
* Захист:
  * метадані поза блоками (переповнення не псує структури аллокатора);
  * вказівники вільних списків закодовані ключем з `AT_RANDOM`;
  * подвійне звільнення і звільнення чужого вказівника виявляються (бітова мапа сторінки) й
    завершують процес `abort` з повідомленням;
  * `calloc` бере вже нульові сторінки від `mmap` без `memset`.
* Пам'ять: порожні сторінки повертаються ядру (`madvise(MADV_DONTNEED)`) з гістерезисом. Ціль --
  ≤ 64 КіБ накладних на маленький процес (уроки frigg).
* Режими через `HXMALLOC=`:
  * `guard` -- кожен блок на окремій сторінці, для налагодження;
  * `junk` -- заповнення звільненого;
  * `stats`.

**mem**:
* `memcpy`/`memmove`/`memset`/`memcmp`/`strlen`/`memchr`/`strchr` -- SSE2-версії як база;
* `rep movsb` на CPU з ERMS, AVX2 лише за `cpuid`;
* вибір один раз при старті (вказівники в RELRO, без IFUNC-магії в rtld).

**io** -- stdio:
* `FILE` з одним буфером і прапорцями як у musl;
* блокування `FILE` вмикається лише після першого `pthread_create` (однопотокові програми не
  платять);
* `fmemopen`, `open_memstream`, `getline`, `getdelim`, `dprintf` -- POSIX.1-2008/2024;
* `fopencookie`/`funopen` -- `funopen` (BSD) у ядрі, `fopencookie` у compat.

**core/fmt** -- printf і scanf:
* власний розбір форматів, повна підтримка C23 (`%b`, `%wN`);
* плаваюча кома:
  * `%g`/`%e`/`%f` з точністю -- точний алгоритм (Ryu-printf / big-int, як musl);
  * найкоротше представлення -- Ryu.
* `%n` заборонено:
  * виклик з `%n` завершує процес, як у OpenBSD;
  * сумісний режим (`HXC_ALLOW_PERCENT_N=1`) -- лише для тестів.
* scanf -- повний C23 і POSIX (`%m`, набори `[...]`, `%n`).
* Уся логіка -- `#![forbid(unsafe_code)]`, фазинг проти musl.

**core/num**:
* `strtod`/`strtof`/`strtold` -- Eisel-Lemire з повільним точним запасним шляхом (правильне
  округлення завжди);
* `strtol` і родина, `qsort` (pdqsort, без рекурсії по стеку), `bsearch`.

**core/string, ctype, wchar**:
* `str*`, `mem*` (не гарячі), `strlcpy`/`strlcat`;
* `strsep`, `strcasestr`, `memmem`, `stpcpy`/`stpncpy`, `strndup`, `wcs*`, `mbrtowc`;
* `wcwidth` з таблиць Unicode 16 (згенеровані дані, спільні з `crates/hxvt/width.rs`).

**core/locale**:
* Локалі -- лише `C`/`POSIX` і `C.UTF-8` (будь-яке `xx_YY.UTF-8` приймається як UTF-8 з правилами C).
  `newlocale`/`uselocale`/`*_l` справжні, бо їх вимагають libc++, foot, GLib.
* Повідомлення:
  * `catopen`/`catgets` -- реальні (формат каталогів NetBSD);
  * `gettext` -- у `libintl` як окремий тонкий модуль (`.mo`-файли), бо GLib/GTK його вимагають.
* iconv -- UTF-8/16/32, Latin-1, CP1251, KOI8-U, CP1252 (рідні для українського користувача).
  Більше -- лише за потреби порту.

**core/time**:
* `strftime`, `strptime`, `mktime`;
* `localtime_r` з TZif v2+ (`/usr/share/zoneinfo`), POSIX-рядок `TZ`.
* `clock_gettime` -- через vDSO, коли ядро дасть його (§6).

**thread**:
* 1:1, `clone`;
* м'ютекси, rwlock, condvar, бар'єри, семафори -- на футексах (алгоритми як у musl, перевірені);
* robust і PI-м'ютекси (`PTHREAD_PRIO_INHERIT`, потрібні Chromium);
* скасування потоків -- лише відкладене, без асинхронного;
* `pthread_setname_np`, `pthread_getaffinity_np` -- у compat;
* стек потоку з охоронною сторінкою.

**net** -- клієнт мережі:
* `getaddrinfo`/`getnameinfo`: `/etc/hosts` + DNS-резолвер (UDP, повтор по TCP, `search`,
  `ndots`, IPv6), розбір відповідей -- безпечний Rust;
* `gethostbyname` -- лише в compat;
* без NSS: `/etc/nsswitch.conf` не потрібен.

**users** -- `getpwnam_r`, `getgrouplist`, `getspnam` (лише root), `crypt`:
* SHA-512, bcrypt `$2b$` (рідний для OpenBSD/FreeBSD), yescrypt -- пізніше;
* хеші сумісні з поточними `/etc/shadow`.

**misc BSD:**
* `<err.h>`, `<sys/queue.h>`, `<sys/tree.h>`, `fts(3)`;
* `arc4random*` (ChaCha20, перезасів з `getrandom`), `getentropy`, `explicit_bzero`,
  `timingsafe_bcmp`, `reallocarray`, `recallocarray`, `getprogname`/`setprogname`, `closefrom`,
  `strtonum`, `readpassphrase`, `funopen`.

**posix_spawn** -- основний спосіб запуску:
* `posix_spawn_file_actions_addchdir`, `addfchdir`, `addclosefrom_np`, `POSIX_SPAWN_SETSID`;
* реалізація через `clone(CLONE_VM|CLONE_VFORK)` з окремим стеком;
* `vfork` -- лише в compat.

**libm** -- порт musl libm (через Rust-крейт `libm`, MIT):
* ті самі результати, що в Linux-шарі, відомі межі похибок;
* `fenv` -- асемблер на архітектуру.

### 4.7 Безпека (усе ввімкнено за замовчуванням)

| Захист | Як |
|---|---|
| Безпека пам'яті реалізації | розбір, форматування, DNS, локалі, crypt -- безпечний Rust; `unsafe` -- лише в `abi/`, `sys/`, `rt/`, `rtld/`, `alloc/raw`, `mem/`; кожен блок `unsafe` -- у базі `lint-unsafe` |
| FORTIFY | `_FORTIFY_SOURCE=2` за замовчуванням у `hx-cc`; `__memcpy_chk`, `__sprintf_chk`, `__read_chk` і т. д. перевіряють `__builtin_object_size` |
| Стек | канарейка з `AT_RANDOM`; `-fstack-clash-protection` у `hx-cc`; охоронні сторінки в потоках |
| Вказівники в пам'яті | `jmp_buf`, `atexit`-список і обробники з rtld зберігаються закодованими (pointer mangling) |
| Завантаження | повний RELRO + `BIND_NOW`; `PT_GNU_STACK` без виконання; для setuid ігноруються `LD_*`, перевіряються fd 0-2 |
| Купа | метадані поза блоками, закодовані вільні списки, виявлення double free, охоронні сторінки для великих блоків |
| Форматні рядки | `%n` заборонено; `-Wformat-security` -- помилка в `hx-cc` |
| Небезпечні функції | `gets` немає; `tmpnam`/`mktemp`/`vfork` -- лише в compat і з попередженням лінкера |
| Випадковість | `arc4random` на ChaCha20 з перезасівом після `fork` |
| Тести | фазинг printf/scanf/strtod/getaddrinfo/strptime/regex проти musl у CI; MIRI для `core` |

### 4.8 Швидкість
* `hello`: від `execve` до `main` -- ≤ 3 `mmap` і жодного `open` поза кешем `ld-hamix.cache`.
* Однопотоковий шлях без атомарних операцій: stdio, `malloc` (кеш потоку), `errno` (TLS).
* `mem*`: SSE2 базово, `rep movsb` на ERMS, вибір при старті; `strlen`/`memchr` -- по 16 байт.
* `qsort` -- pdqsort; `bsearch`, `strstr` -- двосторонній алгоритм (лінійний гарантований).
* `getaddrinfo` -- паралельні запити A/AAAA одним сокетом.
* Час -- через vDSO (`clock_gettime`, `gettimeofday`, `time`).
* Профіль: `libc-bench` (malloc, mem*, printf, strtod, stdio, pthread mutex, spawn) порівнюється з
  musl і поточною mlibc на тому ж ядрі; результати -- в `out/bench/libc.json`, ворота в CI.

### 4.9 Розмір
* Без `core::fmt` у гарячих шляхах (він великий): власне легке форматування для повідомлень
  бібліотеки.
* `opt-level=3` для `mem`, `alloc`, `fmt`, `num`; `opt-level="s"` для решти; LTO;
  `--gc-sections`; `--icf=all`.
* Таблиці (Unicode, часові пояси за замовчуванням) -- стиснені, генеруються xtask-ом.
* Ворота: `cargo xtask size-gate` перевіряє `libc.so.1` ≤ 450 КіБ, `ld-hamix.so.1` ≤ 64 КіБ,
  `libhxcompat.so.1` ≤ 120 КіБ.

### 4.10 Спадщина -- `libhxcompat`

Три яруси, межа між ними перевіряється автоматично:

| Ярус | Бібліотека | Що | Хто користується |
|---|---|---|---|
| Ядро | `libc.so.1` | ISO C, POSIX.1-2024, BSD-розширення (§4.6), `posix_spawn`, `memfd_create`, `eventfd`, `getrandom`, `pipe2`, `dup3`, `accept4`, `ppoll`, `qsort_r`, `strlcpy` | усі |
| Інтерфейси Linux | `libc.so.1`, заголовки `<sys/epoll.h>`, `<sys/inotify.h>`, `<sys/signalfd.h>`, `<sys/timerfd.h>` | обгортки над викликами, які ядро HamixOS справді реалізує (епол, inotify, signalfd, timerfd, `statx`, `prctl`, `unshare`, `sendfile`, `splice`) | libwayland, libinput, GLib, hxstd |
| Спадщина GNU/старого Unix | `libhxcompat.so.1`, заголовки `<compat/*.h>` | `error()`, `program_invocation_name`, `obstack`, `argp`, `backtrace` (execinfo), `mallinfo`, `malloc_usable_size`, `sbrk`, `valloc`, `pvalloc`, `memalign`, `index`/`rindex`/`bcopy`/`bzero`, `tmpnam`, `tempnam`, `mktemp`, `getdtablesize`, `vfork`, `sysinfo`, `__fpurge`, `__fsetlocking`, `fopencookie`, `getutxent`/`utmpx`, `gethostbyname`, `rcmd`, `ftime`, `sigpause`, `siginterrupt`, `*64`-аліаси LFS, `secure_getenv`, `strchrnul`, `mempcpy`, `memrchr`, `canonicalize_file_name`, `pthread_*_np` | Xwayland, X-бібліотеки, PAM, util-linux, частина GNOME-стеку |

* Як підключається:
  * `/usr/lib/libc.so` -- лінкер-скрипт `GROUP ( /lib/libc.so.1 AS_NEEDED ( /lib/libhxcompat.so.1 ) )`
    (glibc робить так само з `libc_nonshared.a`);
  * програма, що не використовує жодного символу спадщини, не отримує `NEEDED libhxcompat.so.1`
    і не платить ні байтом;
  * той, хто використовує, підтягує її автоматично, без змін у рецептах портів.
* Правило: `hxc-compat` реалізує все **поверх публічного C API** ядра libc. Жодних внутрішніх
  гачків, ядро про compat не знає.
* Облік спадщини:
  * `cargo xtask compat-report` показує, які програми образу тягнуть `libhxcompat` і які символи;
  * мета на кожен реліз -- список не росте.
* Інтерфейси Linux -- окремий ярус, бо це не «сміття», а реальні виклики ядра HamixOS (FreeBSD теж
  має `eventfd`, `timerfd`, `memfd_create`, `inotify`).
  Довгостроково -- `kqueue` у ядрі (§6, F9) як рідний BSD-механізм подій; `epoll` тоді переїде до
  compat-ярусу як обгортка (як `epoll-shim` у FreeBSD).

### 4.11 Тестування
1. `cargo test` на хості для `hxc-core`, `hxc-alloc`, `hxc-io` (бекенд `linux`). Це хвилини, не
   QEMU.
2. **Диференційні тести:**
   * ті самі вхідні дані через нашу функцію і через musl хоста (`x86_64-unknown-linux-musl`);
   * фазинг (`cargo fuzz`) для printf/scanf/strtod/strptime/regcomp/getaddrinfo-парсера.
3. **Відповідність:**
   * `libc-test` (musl, MIT) повністю;
   * `open_posix_testsuite` (LTP, як зовнішній раннер -- код не лінкується);
   * кількість проходжень -- метрика в `out/bench/conformance.json`.
4. `abi-check` для трьох архітектур.
5. Завантажувальні тести QEMU:
   * наявні 58 `tests/boot/*` (bash, Nook, Wayland, GTK, Enlightenment);
   * нові `native-hxc-*` (startup, TLS, dlopen, pthread-стрес, fork+exec-цикли, сигнали з потоками).
6. `libc-bench` + `size-gate` у кожній збірці.

### 4.12 Міграція з mlibc

| Крок | Що переходить на нову hxlibc | Ворота |
|---|---|---|
| M1 | статичні тест-програми `tests/hxlibc/*.c` | `libc-test` ≥ 95 %, `abi-check` зелений |
| M2 | усі Rust-програми (hxstd): hxsvc, hxserver, nook-shell, hxterm, hxutils | 58 boot-тестів, golden Nook, `native-memory` |
| M3 | bash, busybox, lua, sqlite, curl, openssl | тести bash/busybox, `native-net-*` |
| M4 | стек Wayland: libwayland, libxkbcommon, libinput, libevdev, mtdev, libdrm, pixman, freetype, harfbuzz, fontconfig, cairo, pango, fribidi, foot, fcft | `native-shell-wayland`, foot у Nook |
| M5 | GLib, GTK3, gdk-pixbuf, atk, D-Bus, Mesa, libglvnd, libepoxy, EFL, Enlightenment | `native-gtk*`, `native-enlightenment*` |
| M6 | Xwayland і X-бібліотеки (через `libhxcompat`) | `native-xwayland`, X11-програми у Nook |
| M7 | видалення `hxlibc/legacy-mlibc`, `/lib/ld.hx`, `libhxc.so` | у живому образі `PT_INTERP=/lib/ld.hx` немає ніде |

* Збірка порту під новою libc -- поле `libc = "hxc"` у `HXBUILD`. `hx-cc` вибирає sysroot за ним.
* Порти з обома варіантами живуть у різних префіксах до кінця кроку.
* Зміна дефолту -- після M4.

### 4.13 Rust-сторона
* `hxlibc-sys` зводиться до набору `extern "C"`-оголошень POSIX (як крейт `libc`) і стає
  переносним: на Linux/BSD -- системна libc, на HamixOS -- наша.
* Пізніше:
  * додати ціль `x86_64-unknown-hamix` у крейт `libc` (апстрім);
  * зібрати справжній `std` для HamixOS (Rust-програми без `hxstd`-фасаду).

---

## 5. Частина B: справжній Wayland у Nook

### 5.1 Цільова архітектура

```
                     hxserver (Nook)
 ┌───────────────────────────────────────────────────────────────────────┐
 │ scene/        дерево вікон і шарів, фокус, робочі столи, анімації,    │
 │               пошкодження (damage), композиція, курсор, DRM/KMS-вивід │
 │               Вікно = { рамка Nook, Source }                          │
 │               Source = Hxproto(shm) | Wayland(surface tree) | Internal│
 ├──────────────────────────────┬────────────────────────────────────────┤
 │ frontend/hxproto/            │ frontend/wayland/                      │
 │  поточний hxproto v2         │  wire.rs     кодек (спільний з         │
 │  (нативні програми,          │              wlclient: crates/wlwire)  │
 │   nook-shell у режимі        │  client.rs   з'єднання, черга fd,      │
 │   hxserver)                  │              мапа об'єктів, ліміти     │
 │                              │  surface.rs  подвійний стан, commit,   │
 │                              │              субповерхні, ролі         │
 │                              │  shm.rs      пули mmap, буфери         │
 │                              │  seat.rs     pointer/keyboard/touch    │
 │                              │  xdg.rs      xdg_wm_base, toplevel,    │
 │                              │              popup, positioner         │
 │                              │  layer.rs    zwlr_layer_shell_v1       │
 │                              │  output.rs   wl_output, xdg-output     │
 │                              │  data.rs     буфер обміну, DnD,        │
 │                              │              primary selection         │
 │                              │  extra.rs    decoration, cursor-shape, │
 │                              │              viewporter, activation,   │
 │                              │              foreign-toplevel,         │
 │                              │              presentation-time         │
 │                              │  xwm.rs      Xwayland: XWM у процесі   │
 └──────────────────────────────┴────────────────────────────────────────┘
```

Ключова відмінність від моста: фронтенд Wayland **не надсилає повідомлень** у hxserver, а
викликає методи сцени. Сцена теж не надсилає подій мосту, а викликає трейт `Frontend`:

```rust
pub trait Frontend {
    fn pointer_enter(&mut self, view: ViewId, x: Fixed, y: Fixed);
    fn pointer_motion(&mut self, view: ViewId, time: u32, x: Fixed, y: Fixed);
    fn pointer_button(&mut self, view: ViewId, time: u32, button: u32, pressed: bool);
    fn key(&mut self, view: ViewId, time: u32, key: u32, pressed: bool, mods: Modifiers);
    fn configure(&mut self, view: ViewId, size: (i32, i32), states: States);
    fn close(&mut self, view: ViewId);
    fn frame_done(&mut self, time: u32);
}
```

### 5.2 Буфери без копіювання
* `wl_shm_pool` -- `mmap` файла клієнта (`MAP_SHARED`, лише читання) у процесі hxserver.
* Після `commit` вікно сцени тримає `BufferRef { pool, offset, stride, format, size }`, і композиція
  читає пікселі **прямо з пам'яті клієнта**.
* `wl_buffer.release` надсилається, коли буфер замінено новим (модель Weston pixman-рендерера).
  Клієнти з подвійним буфером (усі тулкіти) працюють без очікування.
* **Захист від усічення пулу:**
  * розмір пулу перевіряється через `fstat` при створенні і в `resize`;
  * ядро гарантує, що сторінки memfd, на які є відображення, живуть, доки живе відображення
    (`F_SEAL_SHRINK` не потрібен);
  * якщо доступ за межі все ж стається, обробник `SIGBUS` підставляє нульову сторінку
    (`MAP_FIXED`) і позначає клієнта як порушника -- як `wl_shm_buffer_begin_access` у libwayland.
* Формати: `ARGB8888`, `XRGB8888` -- без перетворення; `ABGR`/`XBGR` -- перетворення під час
  композиції.
* Пошкодження: `wl_surface.damage_buffer` → `Area` в координатах екрана. Перемальовується лише
  перетин із видимими областями (наявний `visible_plan`).
* Пізніше (F8): `zwp_linux_dmabuf_v1` для GPU-клієнтів, коли DRM-драйвер дасть PRIME.

### 5.3 Протоколи

| Протокол | Версія | Етап |
|---|---|---|
| `wl_display`, `wl_registry`, `wl_callback`, `wl_compositor` (`wl_surface`, `wl_region`) | 6 | W1 |
| `wl_shm` (`wl_shm_pool`, `wl_buffer`) | 2 | W1 |
| `xdg_wm_base` (`xdg_surface`, `xdg_toplevel`, `xdg_popup`, `xdg_positioner`) | 6 | W1 |
| `wl_seat` (`wl_pointer` з frame/axis_value120, `wl_keyboard` з keymap xkb і repeat_info) | 9 | W1 |
| `wl_output`, `zxdg_output_manager_v1` | 4 / 3 | W1 |
| `zxdg_decoration_manager_v1` (рамка Nook за замовчуванням) | 1 | W1 |
| `zwlr_layer_shell_v1` (nook-shell) | 4 | W2 |
| `zwlr_foreign_toplevel_manager_v1` + `hamix_shell_v1` (геометрія для доку) | 3 / 1 | W2 |
| `wp_cursor_shape_manager_v1`, `wl_pointer.set_cursor` | 1 | W2 |
| `wl_subcompositor` | 1 | W2 |
| `wl_data_device_manager` (буфер обміну, DnD), `zwp_primary_selection_device_manager_v1` | 3 / 1 | W3 |
| `wp_viewporter`, `wp_fractional_scale_manager_v1` | 1 | W3 |
| `xdg_activation_v1`, `wp_presentation`, `xdg_toplevel_icon_v1` | 1 | W3 |
| `zwp_relative_pointer_manager_v1`, `zwp_pointer_constraints_v1` (ігри) | 1 | W4 |
| `zwp_text_input_v3` (методи вводу) | 1 | W4 |
| `xwayland_shell_v1` + XWM у процесі | 1 | W4 |
| `zwp_linux_dmabuf_v1`, `wp_linux_drm_syncobj_v1` | 4 / 1 | F8 |

### 5.4 Кадри і ввід
* `wl_surface.frame`-колбеки виконуються **після композиції, у якій поверхня була видима**. Відлік
  часу -- той самий годинник, що в анімацій hxserver.
  * приховані й згорнуті вікна колбеків не отримують, тобто не малюють даремно;
  * це головна економія процесора на слабкому залізі.
* Ввід:
  * сцена визначає ціль (вікно або шар) і викликає `Frontend::pointer_*` / `key`;
  * координати локальні для поверхні, у `wl_fixed`;
  * неявне захоплення кнопки миші (implicit grab), як у протоколі.
* Клавіатура:
  * keymap xkb генерується один раз (наявна `keys.rs` → таблиця evdev);
  * `memfd` із keymap ділиться між клієнтами;
  * повтор клавіш -- на стороні клієнта (`repeat_info`).
* Декорації: рамку малює Nook (як для hxproto-вікон) для клієнтів з
  `zxdg_decoration_manager_v1`. CSD-клієнти (GTK без протоколу) отримують лише тінь.

### 5.5 Xwayland
* XWM переїжджає з моста в `frontend/wayland/xwm.rs`: X-вікно асоціюється з `wl_surface` через
  `xwayland_shell_v1` (серійний номер), а не через атом `WL_SURFACE_ID`.
* Xwayland запускається ліниво при першому підключенні до `:0` (сокет слухає hxserver).

### 5.6 Межі довіри
* Ліміти на клієнта:
  * об'єкти ≤ 65 536;
  * пули ≤ 512 МіБ сумарно;
  * повідомлення ≤ 4 КіБ;
  * черга вихідних подій ≤ 4 МіБ (інакше клієнт відключається, сервер не блокується).
* `zwlr_layer_shell_v1`, `zwlr_foreign_toplevel_manager_v1`, `hamix_shell_v1` видно лише клієнтам
  з довіреними правами: перевірка `SO_PEERCRED` → член групи `nook-shell` або сама оболонка, яку
  запустив hxserver.
* Розбір повідомлень -- безпечний Rust (`Reader` з перевіркою меж); `unsafe` лише в `mmap`/`recvmsg`.

### 5.7 Що зникає
* `userland/desktop/hxwayland` (lib + bin) і `wayland::Embedded` -- після W4.
* `crates/wlclient` ділить кодек з сервером через новий `crates/wlwire`.
* Змінна `NOOK_WAYLAND=external` -- видаляється.

### 5.8 Перевірка
* Наявні тести: `native-wayland-*`, `native-shell-wayland`, `native-enlightenment-apps` (hxtoolkit
  через Wayland), foot, GTK3-Wayland, Xwayland, golden.
* Нові тести:
  * `native-wayland-zero-copy`: лічильник копій у `hxctl stats` має бути 0;
  * `native-wayland-frame`: згорнуте вікно не отримує колбеків;
  * `native-wayland-limits`: зловмисний клієнт -- переповнення, усічення пулу, 100 000 об'єктів --
    відключається, сервер живий.
* Заміри (`kbench`): затримка від руху миші до кадру та CPU на кадр із foot і GTK-програмою --
  порівняння «міст» vs «нативно».

---

## 6. Частина C: ядро -- передумови швидкості

1. **Спільний кеш сторінок файлів (головний пріоритет):**
   * read-only `MAP_PRIVATE` і `MAP_SHARED` сегменти файлів відображаються на **ті самі фізичні
     сторінки** для всіх процесів;
   * сторінки належать кешу вузла VFS із підрахунком посилань;
   * копія робиться лише при записі (COW);
   * `lazy_fault` перестає читати файл під `VFS.lock()` для вже кешованих сторінок.
   Очікуваний ефект: старт GTK-програми -- з копіювання десятків МіБ до заповнення PTE; менше
   пам'яті (одна копія `libc.so`, Mesa, GTK на всю систему); менше конкуренції за великий замок.
2. **vDSO**: `clock_gettime`, `gettimeofday`, `time`, `getcpu` без системного виклику (сторінка даних
   часу, яку оновлює таймер).
3. **Дрібнозернисті замки** (план U10/U11): кеш сторінок і `lazy_fault` поза BKL, далі --
   fd-таблиця і VFS-читання.
4. **Футекси:** `FUTEX_WAIT_BITSET`, PI-футекси, `futex_waitv` -- для нової `thread` і Chromium.
5. **memfd:** гарантія життя сторінок під відображенням (§5.2), `F_ADD_SEALS`.
6. Пізніше (F9): `kqueue`/`kevent` як рідний механізм подій поруч з `epoll`.

---

## 7. Частина D: вхід без LightDM і X11

### 7.1 `hxdm` -- менеджер входу
* Невеликий Rust-демон (`userland/services/hxdm`, ціль ≤ 300 КіБ), служба hxsvc замість `lightdm.svc`.
* Відповідальність:
  1. взяти сидіння і VT (через `hx-login1`, як logind);
  2. запустити hxserver у режимі привітання (`hxserver --greeter`: без оболонки, одне
     повноекранне вікно, жодних інших клієнтів);
  3. запустити програму привітання від непривілейованого користувача `_greeter`;
  4. автентифікувати через `hx-authd` (`crypt(3)`, `/etc/shadow`; PAM не потрібен);
  5. запустити обраний сеанс (`/usr/share/wayland-sessions/*.desktop`) від імені користувача з
     правильними `XDG_RUNTIME_DIR`, `XDG_SESSION_*`, групами й лімітами;
  6. дочекатися кінця сеансу і повернути привітання.
* **Протокол -- сумісний з greetd** (JSON із префіксом довжини через `$GREETD_SOCK`:
  `create_session`, `post_auth_message_response`, `start_session`, `cancel_session`). Будь-яке
  greetd-привітання (tuigreet, gtkgreet) працює без змін, а наше власне -- це звичайна hxtoolkit-
  програма.
* Програма привітання `nook-greeter`:
  * hxtoolkit, той самий вигляд, що Nook: шпалери, годинник, вибір користувача з аватаром,
    вибір сеансу (Nook / Nook Wayland / Enlightenment), кнопки живлення, розкладка;
  * пам'ятає останній сеанс у `/var/lib/hxdm/state`.
* Зміна сеансу без перезавантаження: hxserver привітання звільняє DRM-master, сеанс бере його; після
  виходу -- навпаки. Те, що зараз робиться через `adopt_vt` і повтори `Screen::take`, стає явним
  передаванням через `hx-login1` (`TakeDevice`/`ReleaseDevice`, як logind).

### 7.2 Що виходить з базового образу
* LightDM, lightdm-gtk-greeter, Xorg, xf86-input-libinput, libxklavier, libpciaccess, linux-pam,
  GTK3-X11-бекенд, `/usr/libexec/hamix/lightdm-*`.
* Профіль збірки `--x11=xwayland` (за замовчуванням): Xwayland + X-бібліотеки + `libhxcompat` для
  X11-програм у Nook. `--x11=none` -- система взагалі без X11 (найменший образ для слабкого заліза).

---

## 8. Частина E: власний завантажувач `hxboot`

### 8.1 Чому не GRUB
* **Повільно на справжньому залізі.** GRUB читає `hext.img` (323 МіБ) через BIOS `INT 13h` з флешки
  чи DVD -- хвилини на R428, а на частині BIOS -- обрив (фото 2026-10-05, U10). Ядру потрібні лише
  `kernel.bin` (~2,6 МіБ) і `initfs.tar`; корінь воно вже вміє знайти саме (живий корінь з USB/ISO,
  U10).
* **Збірка лише на Linux.** `grub-mkrescue` немає в пакетах OpenBSD, а ISO без нього не зібрати.
* **Немає ланцюжка довіри.** GRUB не перевіряє, що завантажив саме наше ядро; підмінене ядро на
  флешці -- повний доступ до всіх даних.
* **Зайва складність:** від GRUB ми беремо 5 тегів multiboot2 (командний рядок, модулі, карта пам'яті,
  framebuffer, RSDP) і меню з 4 пунктів.

### 8.2 Що таке hxboot
* Rust (`no_std`), мінімум асемблера, каталог `boot/hxboot`:
  * `hxboot-core` -- спільне: розбір `hxboot.conf`, меню, ELF64-завантажувач, побудова multiboot2-info,
    перевірка підпису, читання ISO 9660/FAT/hext лише для читання;
  * `hxboot-bios` -- x86 BIOS: сектор 0 (MBR / El Torito no-emulation, 512 Б асемблера) → stage2 у
    32-бітному захищеному режимі з поверненням у реальний режим лише для `INT 13h` (LBA, читання
    блоками по 64 КіБ), `INT 15h E820`, VBE;
  * `hxboot-efi` -- UEFI x86_64 (`x86_64-unknown-uefi`): GOP, `SimpleFileSystem`, `GetMemoryMap`,
    `ExitBootServices`, перехід у 32-бітний режим без сторінок і вхід у ядро як multiboot2 (ядро не
    змінюється);
  * `hxboot-install` -- запис у MBR/розділ BIOS Boot і на ESP для встановленої системи (замість
    `grub-install` у Hamix Install).
* **Протокол із ядром -- multiboot2** (той самий `_start`, ті самі теги), тож GRUB і hxboot
  взаємозамінні під час переходу. Пізніше -- власний протокол `hxboot` (64-бітний вхід одразу, KASLR,
  карта пам'яті без перетворень).
* **Швидкість:**
  * за замовчуванням вантажить лише ядро + initfs; `hext.img` -- тільки для пункту «копія в RAM»;
  * читання великими блоками й одночасний розбір ELF;
  * меню з таймаутом 1 с (0 -- якщо не натиснуто Shift), без шрифтів і тем;
  * ціль: від передачі керування BIOS до `_start` ядра ≤ 300 мс з USB 2.0 (GRUB зараз -- секунди).
* **Конфігурація** `/boot/hxboot.conf` -- рядки `entry "назва" kernel=... initfs=... cmdline=...`,
  без мови сценаріїв.
* **ISO і флешка без GRUB:** xtask сам будує образ El Torito (BIOS) + ESP (FAT12, власний запис у
  xtask, як `exfat.rs`) і пише ISO через `xorriso -as mkisofs` (є в пакетах Linux і OpenBSD); образ
  гібридний (MBR + GPT), як зараз.

### 8.3 Безпечне завантаження
* Ядро й initfs підписуються ключем Ed25519 при збірці (`cargo xtask build`, ключ -- `HAMIX_SIGNING_KEY`
  або ключ розробника в `~/.config/hamix/`). Публічний ключ вбудований у hxboot.
* hxboot перевіряє підпис перед передачею керування; непідписане ядро -- лише з явного пункту меню
  «unsigned kernel» і червоним попередженням (для розробки), у режимі `hxboot.secure=1` -- відмова.
* Виміряне завантаження: hxboot передає ядру SHA-256 ядра, initfs і командного рядка (тег
  `hamix_measurements`), ядро показує їх у `/proc/hxinit` і `hwreport`.
* UEFI Secure Boot -- пізніше: підпис `hxboot.efi` ключем користувача (MOK) або власним PK.

### 8.4 Ventoy
Ventoy -- найпоширеніший спосіб, яким користувачі запускають наш ISO на залізі (фото з R428, U10).
hxboot має працювати з ним краще, ніж GRUB:
* **Обидва режими Ventoy:**
  * звичайний (Ventoy підставляє ISO як віртуальний CD: у BIOS -- через перехоплення `INT 13h`, в
    UEFI -- як `BlockIo` з El Torito): hxboot читає власні файли з ISO 9660 сам, через `INT 13h` або
    `BlockIo`, не покладаючись на `SimpleFileSystem` чи розмір сектора, який повідомив BIOS (2048 і 512);
  * GRUB2-режим і memdisk Ventoy: hxboot.efi і `boot/grub/grub.cfg` з пунктом `chainloader`/`linux16`
    на hxboot лишаються в ISO, тож будь-який режим доходить до hxboot.
* **Таблиця `ventoy_os_param`** (GUID `77772020-2e77-6576-6e74-6f792e6e6574`, BIOS -- у пам'яті
  `0x80000-0xA0000`, UEFI -- змінна `VentoyOsParam`): hxboot знаходить її, перевіряє контрольну суму
  і передає ядру тегом `hamix_ventoy` (диск за GUID/підписом MBR, розділ, шлях ISO, список
  фрагментів файла).
* **Ядро з цим тегом** відкриває ISO на флешці напряму за списком фрагментів
  (`block::register_mapped_disk`, U10) -- без пошуку `*hamix*.iso` по exFAT/FAT, без читання
  `hext.img` через BIOS, і з NTFS-розділом Ventoy теж (ядро не мусить розуміти NTFS, бо фрагменти
  вже відомі). Без тегу -- поточний пошук як запасний шлях.
* **`ventoy.json` і плагіни:** працюють пункти `menu_alias`, `persistence` (файл `persistence.dat`
  монтується як `/home` з міткою `casper-rw`/`hamix-rw`) і `injection` (архів розпаковується в
  initfs-оверлей).
* **Secure Boot Ventoy** (Ventoy з MOK): `hxboot.efi` підписаний ключем, який можна додати через
  `MokManager` Ventoy.
* **Тести:** `native-ventoy` розширюється: справжній Ventoy (BIOS і OVMF), exFAT і NTFS, фрагментований
  ISO, persistence; швидкість від меню Ventoy до `login:`.

### 8.5 Ризики
* BIOS-сумісність: GRUB має 20 років обхідних шляхів. Запобіжник -- GRUB лишається другим пунктом у
  ISO (`--loader grub|hxboot|both`, типово `both`, поки hxboot не пройде R428 і 3 інші машини).
* Тести: QEMU SeaBIOS (CD, USB EHCI/UHCI, IDE), OVMF (UEFI), Ventoy-подібна флешка (`native-ventoy`).

## 9. Частина F: захист від дір і перехоплення даних

Модель загроз: (1) шкідлива або зламана програма користувача намагається прочитати чужі дані
(паролі, вміст вікон, натискання клавіш, файли інших користувачів) або піднятися до ядра; (2) дірка в
ядрі чи libc дає виконання коду; (3) фізичний доступ до вимкненого ноутбука чи флешки.

### 9.1 Ядро: експлуатація
| Захист | Стан | Що зробити |
|---|---|---|
| W^X для програм | є (U10) | aarch64/riscv64 |
| W^X для ядра | є (S1): `.text` R-X, `.rodata` R--, дані й прямий мапінг RW- NX | aarch64/riscv64 |
| SMEP / SMAP | SMEP є (S1) | SMAP: `usercopy` під `stac`/`clac` (з Broadwell) |
| UMIP | є (S1) | -- |
| KASLR | ні: ядро з 1 МіБ | ядро PIE, hxboot вибирає зсув (§8.2) |
| ASLR програм | ні: `USER_MMAP_BASE` фіксований | випадкові бази стека, `mmap`, PIE, `brk`, vDSO (`AT_RANDOM` уже є) |
| Стек ядра | 256 КіБ без охорони | охоронна сторінка для кожного стека ядра (vmalloc), `-Z stack-protector=strong` |
| Купа ядра | карантин при пошкодженні (U10) | обнуління при звільненні для блоків з даними користувача; `ENOMEM` замість паніки (U10) |
| Spectre/Meltdown | ні | KPTI для CPU без `RDCL_NO`, `IBRS`/retpoline для непрямих викликів у системних викликах, `lfence` на межі `usercopy` |
| `unsafe` | `lint-unsafe` | `unsafe_op_in_unsafe_fn = deny`, зменшення бази щорелізу |
| Фазинг | `native-syscall-fuzz` | багатопотоковий, довгий нічний прогін; syzkaller-подібний опис викликів |

### 9.2 Ядро: перехоплення даних між процесами
* **Ввід:** evdev віддає події лише клієнтам активного VT (є); додатково -- лише власнику сеансу на
  сидінні (`hx-login1`), `EVIOCGRAB` лише для композитора. Програми Nook не мають доступу до
  `/dev/input` взагалі (група `input` -- лише в hxserver).
* **Екран:** знімки екрана і запис -- лише через запит до hxserver з діалогом підтвердження
  (`hamix_screencopy` з дозволом `screen`), `/dev/fb0` і `/dev/dri/card0` -- лише DRM-master.
* **Пам'ять інших процесів:** `ptrace` уже `EPERM`; `/proc/<pid>/{environ,maps,mem}` -- лише власнику
  (є для `environ`), `hidepid=2` для `/proc` (чужі процеси невидимі звичайному користувачу).
* **Спільна пам'ять:** `shm_open` у власному просторі імен користувача; memfd -- лише за fd.
* **Сокети:** `$XDG_RUNTIME_DIR` 0700 (перевірка ядром при створенні), `SO_PEERCRED` у всіх службах.
* **Буфер обміну:** читання лише вікном у фокусі (модель Wayland) -- і для hxproto.
* **Звук/мікрофон/камера:** дозвіл групи (U7a) + індикатор у панелі Nook, коли пристрій відкрито.
* **Обнуління:** сторінки, повернені ядру, обнулюються до видачі іншому процесу (перевірити всі шляхи:
  `munmap`, кеш файлів, сокетні буфери, pipe).

### 9.3 Обмеження програм (як `pledge`/`unveil` в OpenBSD)
* Виклики `hx_pledge(promises)` і `hx_unveil(path, perms)` у ядрі, обгортки `pledge(2)`/`unveil(2)` у
  новій hxlibc з тими самими іменами обіцянок (`stdio rpath wpath inet unix proc exec ...`), щоб код з
  OpenBSD збирався без змін.
* Усі власні служби й програми базової системи (hxsvc, hx-authd, hx-netd, hxdm, hxserver, nook-shell,
  hxterm, hxfiles...) викликають `pledge`/`unveil` після ініціалізації. Порушення -- `SIGABRT` і
  запис у журнал.
* Дозволи програм (U7a) переходять з груп на `unveil`-профілі в `.desktop` (`X-Hamix-Unveil`).

### 9.4 hxlibc
Усе з §4.7, плюс:
* `pledge`/`unveil`, `explicit_bzero` для паролів у `readpassphrase`, `crypt`, `getpass`;
* `malloc` обнулює блоки при звільненні (`HXMALLOC=junk` -- за замовчуванням для setuid-програм);
* `getaddrinfo` перевіряє ID і порт відповіді DNS, випадковий порт джерела (захист від підміни);
* TLS-клієнт для системних програм -- rustls, перевірка сертифікатів обов'язкова (жодного
  `curl -k` у службах).

### 9.5 Дані на диску
* Шифрування домашнього каталогу (hext + XChaCha20-Poly1305 на рівні блоків, ключ з пароля входу
  через Argon2id), розблокування в `hxdm`.
* Swap (коли з'явиться) -- лише зашифрований випадковим ключем.

### 9.6 Перевірка
* Тест `native-hardening`: SMEP/SMAP увімкнені, сторінки ядра не RWX, ASLR (дві програми -- різні адреси),
  `/proc` інших користувачів невидимий, evdev недоступний поза сеансом, `pledge` вбиває порушника.
* `cargo xtask audit`: `lint-unsafe`, перелік setuid-файлів образу (лише очікувані), права `/dev`,
  відсутність RWX-сегментів в ELF образу.

---

## 10. Частина G: `hsh` замість bash

### 10.1 Чому
* bash у базовому образі -- 1,3 МіБ самого `bash` плюс readline і ncurses, GPL-3, C з історією дірок
  (Shellshock). Він -- оболонка `root` і `user` (`/etc/passwd`), від нього залежать 9 boot-тестів.
* `userland/core/hsh` (5,5 тис. рядків Rust, `no_std` на hxstd) уже є, але вміє лише просту частину POSIX і
  тому відійшов на другий план.
* Рішення: **повертаємось до hsh і видаляємо bash з базової системи.** hsh має бути **максимально сумісним
  з bash**, щоб скрипти, `.bashrc` і звички користувачів працювали без змін. bash лишається лише рецептом у
  `ports/` як необов'язковий пакет.

### 10.2 Що має вміти hsh
* **POSIX.1-2024 sh повністю:** граматика XCU (усі складені команди, функції, here-doc з `<<-`, усі розширення
  параметрів, арифметика `$(( ))`, поля й IFS, `set -eufnvx`, `trap`, спецвбудовані команди з правилами
  POSIX). Як `/bin/sh` (або з `--posix`) -- строгий режим, як `bash --posix`.
* **Розширення bash** (за частотою в реальних скриптах):
  * `[[ ]]` з `=~` (ERE, `BASH_REMATCH`), `==` з шаблонами, `-v`, `<`/`>`;
  * масиви: індексовані й асоціативні, `${a[@]}`, `${!a[@]}`, `${#a[@]}`, `a+=()`, зрізи;
  * розширення: `${v//p/r}`, `${v/#p/r}`, `${v:o:l}`, `${v^^}`/`${v,,}`, `${!name}`, `${!prefix*}`, `${v@Q}`;
  * `$'...'`, фігурні дужки `{a,b}`/`{1..10..2}`, `<(...)`/`>(...)`, `<<<`, `&>`, `|&`, `(( ))`, `let`;
  * `local`, `declare`/`typeset` (`-a -A -i -r -x -g -n -l -u`), `readonly`, `source`, `mapfile`/`readarray`,
    `printf -v` і `%q`, `read -r -a -p -s -t -n -d -u`, `select`, `shopt` (`extglob`, `nullglob`, `globstar`,
    `dotglob`, `nocaseglob`, `failglob`, `lastpipe`, `expand_aliases`), `set -o pipefail`;
  * `trap` з `ERR`, `EXIT`, `RETURN`, `DEBUG`; `PIPESTATUS`, `FUNCNAME`, `BASH_SOURCE`, `LINENO`, `RANDOM`,
    `SECONDS`, `EPOCHSECONDS`, `BASHPID`, `SHLVL`;
  * керування задачами: `jobs`, `fg`, `bg`, `disown`, `wait -n`, `kill %1`; `coproc` -- пізніше.
* **Інтерактивність як у bash:** редактор рядка (emacs і vi), історія (`~/.hsh_history`, читає й
  `~/.bash_history`), `Ctrl-R`, `!!`/`!$`, `PS1` з екрануваннями bash (`\u \h \w \$ \[ \]`), автодоповнення
  файлів і команд + `complete`/`compgen` (щоб працювали готові скрипти bash-completion).
* **Сумісність запуску:** `/bin/sh` і `/bin/bash` -- посилання на hsh; викликаний як `bash`, hsh читає
  `~/.bashrc`/`~/.bash_profile` і ставить `BASH_VERSION`/`BASH_VERSINFO` (5.2) для скриптів, що їх перевіряють;
  `HSH_VERSION` -- справжня версія.
* **Безпека:** безпечний Rust (без `unsafe` поза hxport), функції не експортуються через змінні оточення
  (жодного Shellshock), `pledge`/`unveil` для `hsh -r` (обмежена оболонка).

### 10.3 Перенесення й тести
* hsh переходить з `no_std` + hxstd на `hxport` (std на Linux/BSD, hxstd на HamixOS) -- збирається й
  тестується на хості без QEMU.
* **Диференційні тести проти справжнього bash на хості:** spec-тести Oils (oils-for-unix, Apache-2.0, ~2000
  випадків для bash/dash/mksh), набір тестів bash як зовнішній раннер (код bash не лінкується), POSIX-тести
  yash. Метрика -- частка випадків, у яких hsh дає той самий вивід і код, що bash; ворота -- ≥ 95 % для
  категорій, які проходить bash, 100 % для POSIX.
* Фазинг парсера (`cargo fuzz`): жодної паніки на будь-якому вході.
* Boot-тести: 9 тестів, що зараз викликають bash, переходять на hsh; новий `native-hsh-bash` запускає
  скрипти з образу (`install.sh`, `hwreport`, обгортки сесій) і bash-completion.

### 10.4 Цілі
| Метрика | bash зараз | hsh ціль |
|---|---|---|
| розмір (stripped) | 1,3 МіБ + readline + ncurses | ≤ 600 КіБ, без залежностей крім libc |
| старт `sh -c true` | вимірюємо | ≤ bash / 2 |
| RSS інтерактивної оболонки | вимірюємо | ≤ 1 МіБ |
| Oils spec (bash-категорії) | -- | ≥ 95 % збігів з bash |

---

## 11. Етапи

Позначки: `[ ]` -- не почато, `[~]` -- іде, `[x]` -- готово.

### H1. Збірка на OpenBSD (поруч з усім іншим)
- [x] `cargo xtask kernel` stable rustc без rustup (`RUSTC_BOOTSTRAP`, `rust-src` у `target/rust-src`,
      shim `rust-lld` → `ld.lld -no-pie --no-execute-only`), `--target host-tuple`
- [x] `tools/openbsd/tune-build.sh`: клас `hamixbuild`, `hw.smt`, `kern.pool_debug`, `noatime`, пакети
- [x] `cargo xtask build --loader hxboot` повністю (2026-10-10): кроки GRUB необов'язкові, `install`/`cp -al`/
      `head -c`/`sha256sum` -- код xtask, builtins compiler-rt з Alpine, freestanding-заголовки для C-модулів,
      `hx-cc` з `--no-execute-only`, ISO пише сам xtask (без `xorriso`)
- [ ] `cargo xtask hxlibc-build` і `cargo xtask ports` на OpenBSD (Meson/ninja, GNU make/sed у рецептах)
- [~] `cargo xtask test` у QEMU на OpenBSD: TCG, тайм-аути ×4 (`HX_TCG_FACTOR`); тести, яким бракує
      `mkfs.exfat`/`mkfs.vfat`/mtools, пропускаються з поясненням

### B1. hxboot: BIOS + ISO без GRUB (2 тижні)
- [x] `boot/hxboot` (BIOS, 32 КіБ): El Torito і гібридний MBR, `INT 13h` LBA, E820, VBE, RSDP, ELF64,
      multiboot2-info, ISO 9660, меню з таймаутом і редагуванням командного рядка (`e`), `reboot`
- [x] Пошук інших ОС (`probe yes`: Windows, BSD, Linux з GRUB у розділі, FAT DOS/Windows, інший диск
      цілком) і ланцюгове завантаження `chain signature=... partition=N` (диск -- за підписом MBR)
- [x] `hxbootctl` (`list/show/default/timeout/add/add-chain/add-reboot/module/cmdline/rename/remove/move/
      probe --add/auto/check/edit`), документований `/boot/hxboot.cfg` для ручного редагування,
      довідник `docs/reference/hxboot.md`; тест `native-hxbootctl-fakeos`
- [x] xtask: `--loader grub|hxboot` (типово GRUB, якщо є `grub-mkrescue`), власний запис ISO 9660 + Joliet +
      El Torito + гібридний MBR (`isoimage.rs`); тестові ISO теж через нього
- [x] Ventoy (BIOS): `ventoy_os_param` → тег `0x48585601`, ядро відкриває ISO за фрагментами без пошуку
      (і на NTFS-флешці); тест `native-vtoy-ntfs` (5 фрагментів у зворотному порядку)
- [ ] Ворота: усі boot-тести через hxboot у SeaBIOS (CD, IDE, USB EHCI/UHCI), `native-ventoy` зі справжнім
      Ventoy, час до `_start` ≤ 300 мс

### B2. hxboot: UEFI, встановлена система, підпис (2 тижні)
- [~] Перший x86_64 UEFI шлях: GRUB EFI у El Torito для ISO з hxboot, multiboot2 і GOP framebuffer;
      OVMF-перевірка ще не запускалась. Далі: власний `hxboot-efi` (GOP, файли ESP, передача ядру
      у 32-бітному режимі), запуск встановленої системи з ESP, пряме UEFI-завантаження з USB та OVMF-тести
- [ ] `hxboot-install` замість `grub-install`; Hamix Install на hxboot
- [ ] Підпис ядра й initfs Ed25519, перевірка в hxboot, тег вимірювань для ядра
- [ ] Ventoy (UEFI): `BlockIo` віртуального CD, змінна `VentoyOsParam`, persistence та injection, NTFS-флешка
- [ ] Ворота: R428 і ще 3 машини; після цього GRUB -- лише `--loader grub`

### S1. Ядро: W^X, SMEP/SMAP, охорона стеків (1-2 тижні)
- [~] SMEP і UMIP (CR4 на кожному CPU, `nosmep`/`noumip` у командному рядку) -- зроблено; W^X ядра (2026-10-12):
      `.text` R-X, `.rodata` R--, дані й прямий мапінг RW- NX, `EFER.NXE` у трампліні AP, трамплін без X після
      старту AP, самоперевірка RWX-сторінок у журналі, `nokwx` -- зроблено; SMAP -- ні (потребує переходу всіх
      `user_slice` на копіювання)
- [ ] Охоронні сторінки стеків ядра (стеки 64 КіБ зараз у купі в прямому мапінгу; перед переносом у vmalloc --
      перевірити драйвери на DMA зі стекових буферів); `unsafe_op_in_unsafe_fn = deny`
- [x] Тест `native-hardening` (перша частина: W^X ядра, 0 RWX-сторінок)

### S2. ASLR, KASLR, Spectre (1-2 тижні)
- [ ] Випадкові бази стека, `mmap`, `brk`, PIE-програм; KASLR через hxboot
- [ ] KPTI для вразливих CPU; retpoline/IBRS на вході системних викликів

### S3. Ізоляція даних між процесами (1 тиждень)
- [ ] `hidepid=2`, `/proc/<pid>/{maps,mem}` лише власнику; обнуління всіх сторінок, що повертаються
- [ ] evdev лише власнику сеансу; знімки екрана лише через hxserver з підтвердженням; буфер обміну
      лише вікну у фокусі

### S4. `pledge`/`unveil` (2 тижні)
- [ ] Виклики ядра, обгортки в hxlibc (стара і нова), обіцянки як в OpenBSD
- [ ] Усі служби й програми базової системи під `pledge`/`unveil`; дозволи програм -- `X-Hamix-Unveil`

### S5. Дані на диску (після F6)
- [ ] Шифрування домашнього каталогу, розблокування в `hxdm`; `cargo xtask audit`

### SH1. hsh: POSIX повністю, на хості (2 тижні)
- [ ] hsh на `hxport` (std на Linux/BSD); новий парсер за граматикою XCU, розширення параметрів, арифметика, IFS
- [ ] Раннер spec-тестів Oils і POSIX-тестів проти bash/dash хоста; фазинг парсера
- Ворота: 100 % POSIX-категорій, `/bin/sh` = hsh у boot-тестах

### SH2. hsh: розширення bash (2-3 тижні)
- [ ] `[[ ]]`/`=~`, масиви, `declare`, `$'..'`, фігурні дужки, `<( )`, `<<<`, `(( ))`, `shopt`, `trap ERR/RETURN`,
      `PIPESTATUS`, `mapfile`, `printf -v/%q`, `read` з усіма прапорцями
- Ворота: ≥ 95 % збігів з bash на spec-тестах Oils; скрипти образу (`install.sh`, `hwreport`) під hsh

### SH3. hsh: інтерактивність (1-2 тижні)
- [ ] Редактор рядка (emacs/vi), історія, `Ctrl-R`, `PS1` як у bash, керування задачами, `complete`/`compgen`
- [ ] Читання `.bashrc`/`.bash_profile`, коли викликано як `bash`

### SH4. Видалення bash з базової системи (1 тиждень)
- [ ] `/bin/sh`, `/bin/bash` → hsh; `/etc/passwd` і `useradd` -- `/usr/bin/hsh`; 9 boot-тестів на hsh
- [ ] bash -- лише необов'язковий пакет у `ports/`; ADR «hsh замість bash»
- Ворота: усі boot-тести, golden, розмір образу менший щонайменше на 2 МіБ

### F0. Ядро: спільний кеш сторінок і vDSO (1-2 тижні) -- першим, бо це найбільший виграш швидкості
- [~] Кеш сторінок вузла VFS з підрахунком посилань та окремою генерацією інвалідації на вузол; x86_64 read-only файлові відображення -- спільні сторінки
- [~] COW для приватних відображень при записі; `lazy_fault` обходить `VFS.lock()` для кешованих сторінок
- [~] Обмежений LRU-кеш сторінок (4 МіБ), витіснення лише сторінок без активних відображень
- [~] Початковий x86_64 vDSO: ELF-сторінка з `clock_gettime`, `gettimeofday`, `time`, `getcpu`, карта
      RX і `AT_SYSINFO_EHDR`; функції часу читають спільні дані TSC, а `getcpu` використовує RDTSCP
      та `IA32_TSC_AUX`, із syscall fallback коли RDTSCP недоступний. Збірка ядра проходить; лишилися
      перевірка завантаження у QEMU та заміри.
- [ ] Заміри до/після: старт GTK-програми, foot, RSS системи після входу, `bkl_contended`
- Ворота: усі boot-тести; старт GTK-програми ≥ 2× швидше в QEMU 1 CPU

### W1. Nook Wayland: ядро протоколу (2 тижні)
- [~] `crates/wlwire` і `crates/wlclient`: спільний `no_std` encoder/reader/header decoder, окремий
      від hxproto POSIX-транспорт, коректне передавання fd з частковими записами, ліміти SHM
      (16 384 px / 128 МіБ), автоматичне звільнення локального SHM-відображення та обмежені
      вхідний буфер і черга подій; нативний frontend hxserver ще попереду
- [ ] `frontend/wayland/{client,surface,shm,seat,output,xdg}.rs`; трейт `Frontend` у сцені
- [ ] `Source::Wayland` у вікнах сцени; композиція прямо з пулу клієнта; release при заміні
- [ ] Рамка Nook через `zxdg_decoration_manager_v1`; CSD-клієнти без рамки
- [~] Frame-колбеки: відкладення до позиціювання і фільтр поверхонь поза виходом; повне врахування оклюзії ще потрібне
- Ворота: foot, GTK3-Wayland, hxtoolkit-Wayland у Nook без `hxwayland`; `native-wayland-zero-copy`

### W2. Nook Wayland: оболонка і робочий стіл (1 тиждень)
- [ ] `zwlr_layer_shell_v1`, foreign-toplevel, `hamix_shell_v1` нативно -- nook-shell на Wayland без моста
- [ ] `wl_subcompositor`, `wp_cursor_shape_manager_v1`, курсори клієнтів
- [ ] Перевірка довіри для привілейованих протоколів (`SO_PEERCRED` + група)
- Ворота: `native-shell-wayland`, golden у режимі Wayland

### W3. Nook Wayland: повнота (1-2 тижні)
- [ ] Буфер обміну, DnD, primary selection між Wayland, hxproto і X11
- [ ] `wp_viewporter`, `wp_fractional_scale`, `xdg_activation_v1`, `wp_presentation`
- [ ] Ліміти клієнта, `SIGBUS`-захист пулів, `native-wayland-limits`

### W4. Xwayland і видалення моста (1 тиждень)
- [ ] XWM у `frontend/wayland/xwm.rs` через `xwayland_shell_v1`; лінивий старт Xwayland
- [ ] Видалити `userland/desktop/hxwayland`, `wayland::Embedded`, `NOOK_WAYLAND`
- [ ] ADR-0019B «Nook як Wayland-композитор»
- Ворота: усі boot-тести, X11-програми в Nook, `lint-unsafe` не вище бази

### F1. hxlibc: каркас і рантайм (2 тижні)
- [ ] Workspace `hxlibc/crates/*`, ціль `x86_64-hamix-libc`, `hxlibc-build` без Meson
- [ ] `sys` (hamix + linux бекенди), `rt` (`_start`, auxv, TLS, канарейка), `mem` (SSE2)
- [ ] `hxmalloc` з тестами на хості й стрес-тестом; `libc-bench` і перші заміри vs mlibc і musl
- [ ] Заголовки: `assert.h`, `ctype.h`, `errno.h`, `limits.h`, `stdint.h`, `stddef.h`, `stdlib.h`,
      `string.h`, `stdio.h`, `unistd.h`, `fcntl.h`, `sys/types.h`; `abi-check`
- Ворота: статичний `hello` і `tests/hxlibc/spike.c` у QEMU; `cargo test` на хості

### F2. hxlibc: stdio, форматування, числа, час (2 тижні)
- [ ] printf/scanf/strtod з фазингом проти musl; stdio з ледачим блокуванням
- [ ] `time`, `strftime`/`strptime`, TZif; `locale` (C/C.UTF-8, `*_l`), `wchar`, `iconv` (базові кодування)
- [ ] `libc-test`: частина `functional/` і `regression/` для цих модулів

### F3. hxlibc: процеси, потоки, сигнали, rtld (3 тижні)
- [ ] `posix_spawn`, `fork`, `exec*`, `wait*`, сигнали (`sigaction`, `sigaltstack`, `sigsetjmp`)
- [ ] pthread на футексах, TLS динамічний, cancellation (відкладене), robust/PI м'ютекси
- [ ] `ld-hamix.so.1`: GNU hash, RELR, TLSDESC, `dlopen`, `dl_iterate_phdr`, `ldconfig` і кеш
- [ ] Ворота M1 і M2: усі Rust-програми на новій libc, 58 boot-тестів, golden

### F4. hxlibc: мережа, користувачі, BSD-набір, `libhxcompat` (2 тижні)
- [ ] `getaddrinfo` (hosts + DNS), сокети, `if_nameindex`, `getifaddrs`
- [ ] `pwd`/`grp`/`shadow`, `crypt` (SHA-512, bcrypt), `err.h`, `fts`, `arc4random`, `regex`, `glob`, `wordexp`
- [ ] `hxc-compat` і лінкер-скрипт `AS_NEEDED`; `cargo xtask compat-report`
- [ ] Ворота M3: bash, busybox, curl, openssl, sqlite, lua

### F5. Порти на hxlibc (3-4 тижні)
- [ ] M4 (Wayland-стек, шрифти, foot), M5 (GLib, GTK3, Mesa, D-Bus, EFL/E), M6 (Xwayland + X-бібліотеки)
- [ ] `libc = "hxc"` за замовчуванням у `HXBUILD`; звіт `compat-report` у CHANGES

### F6. Вхід без LightDM (1-2 тижні)
- [ ] `hx-login1`: `TakeDevice`/`ReleaseDevice`, сеанси на сидінні
- [ ] `hxdm` (greetd-протокол) + `hxserver --greeter` + `nook-greeter`
- [ ] Видалити LightDM, Xorg, xf86-input-libinput, libxklavier, linux-pam з базового образу; профілі `--x11`
- [ ] ADR-0021B «hxdm замість LightDM»
- Ворота: вхід у Nook, Nook Wayland, Enlightenment; вихід і повторний вхід; зміна користувача

### F7. Прибирання mlibc (1 тиждень)
- [ ] M7: жодного `PT_INTERP=/lib/ld.hx` в образі; видалити `hxlibc/legacy-mlibc`, `libhxc.so`, `ld.hx`
- [ ] ADR-0020B «hxlibc -- власна libc на Rust» (замінює ADR-0005B)
- [ ] Звіт метрик §2 у CHANGES

### F8. GPU-шлях Wayland (після F7)
- [ ] `zwp_linux_dmabuf_v1`, імпорт dmabuf у композитор, `wp_linux_drm_syncobj_v1`
- [ ] Сканування повноекранного вікна напряму (direct scanout)

### F9. Інші архітектури й BSD-події (в самому кінці, разом із рештою портування)
- [ ] hxlibc `sys`/`rt`/`mem`/`rtld` для aarch64 і riscv64; `abi-check` для них
- [ ] `kqueue`/`kevent` у ядрі; `epoll` -- в compat-ярус

## 12. Ризики

| Ризик | Ймовірність | Запобіжник |
|---|---|---|
| Нова libc ламає великі порти (GLib, Mesa, GTK) тонкими розбіжностями | висока | диференційні тести проти musl; міграція поштучно (§4.12) з поверненням порту на mlibc одним полем `HXBUILD` |
| Rust-код у libc росте в розмірі | середня | `size-gate` у кожній збірці; без `core::fmt` у бібліотеці; `opt-level="s"` для холодних модулів |
| Нульова копія Wayland: клієнт змінює буфер під час композиції | середня | лише візуальний артефакт, без загрози серверу; release при заміні -- клієнти з подвійним буфером не страждають |
| Усічення пулу клієнтом → `SIGBUS` у сервері | середня | гарантія ядра (§6.5) + обробник `SIGBUS` з нульовою сторінкою |
| Спільний кеш сторінок ламає семантику запису в змаповані файли | середня | COW для `MAP_PRIVATE`; тест `MAP_SHARED`-узгодженості (баг E26 з пам'яті проєкту) |
| Відмова від LightDM ламає сценарії, де потрібен X11-вхід | низька | greetd-сумісність; X11-сеанси -- через Xwayland у Nook |
| Обсяг робіт | висока | етапи незалежні: W-гілка (Wayland) і F-гілка (libc) ідуть паралельно; F0 дає виграш одразу |

## 13. Порядок

1. **F0** -- найбільший виграш швидкості для користувача, незалежний від решти. Поруч -- **B1**
   (hxboot: швидке завантаження з флешки на залізі і збірка ISO на OpenBSD) і **S1** (W^X і SMEP/SMAP
   ядра -- дешево, а закриває цілий клас атак).
2. **W1-W4** -- справжній Wayland у Nook (тут уже є основа: протоколи моста переносяться в
   `frontend/wayland`, змінюється модель даних).
3. **F1-F4** -- нова hxlibc; паралельно з W-гілкою, бо зачіпає інші каталоги.
4. **F5-F7** -- переведення портів, `hxdm`, видалення mlibc.
5. **B2, S2-S4, SH1-SH4** -- UEFI і підпис, ASLR/KASLR, ізоляція даних, `pledge`/`unveil`; hsh замість bash
   (SH1 можна починати будь-коли, бо він живе на хості й не залежить від решти).
6. **D1** -- легке робоче середовище на `hxserver`, єдина тема системи й програм.
7. **F8, F9, S5** -- GPU-шлях, інші архітектури, `kqueue`, шифрування диска.

Кожен етап закривається: усі boot-тести, golden, `lint-layers`, `lint-unsafe`, `size-gate`,
оновлені CHANGES і цей файл.

## 14. Легке ретро робоче середовище на `hxserver`

Мета -- проста й швидка DE з трохи ретро інтерфейсом, вбудована в `hxserver`. Вона має меню програм,
віконний інтерфейс і базові налаштування. Власна тема HamixOS поширюється на оболонку та програми,
що використовують `hxproto` і `hxtoolkit`, щоб інтерфейс системи виглядав цілісно.

- [ ] Зафіксувати UX і візуальну систему: читабельність на малих екранах, керування клавіатурою та
      мишкою, мала кількість постійно активних компонентів
- [ ] Зібрати оболонку на `hxserver`: робочий стіл, панель і меню програм, запуск та перемикання вікон,
      фокус, згортання, переміщення й закриття вікон
- [ ] Додати базові налаштування системи: тема, екран, звук, мережа, дата й час, пристрої та сеанс
      через наявні системні служби
- [ ] Створити власну легку тему HamixOS для `hxproto` і `hxtoolkit`, спільні віджети й ресурси;
      перевести базові програми на однакові кольори, шрифти, рамки, кнопки, меню та діалоги
- [ ] Під'єднати каталог `.desktop` програм, іконки й локалізацію; меню показує встановлені програми,
      доступні користувачу
- [ ] Встановити бюджети RAM, розміру образу, часу старту оболонки та CPU у простої; скоротити
      непотрібні фонові процеси та контролювати бюджети автоматично
- [ ] Додати golden-знімки й інтерактивні перевірки меню, вікон, налаштувань і теми; перевірити
      масштабування та роботу на слабкому залізі
- Ворота: базова сесія без LightDM/Xorg, єдина тема в оболонці та базових програмах `hxproto`/
  `hxtoolkit`; дотримані бюджети розміру, пам'яті й часу запуску
