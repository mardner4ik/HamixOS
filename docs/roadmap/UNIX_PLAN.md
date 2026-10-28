# План U: HamixOS як звичайна Unix-подібна система

Status: active · Started: 2026-09-29 · Updated: 2026-10-10 · Далі: `FOUNDATION_PLAN.md` (план F: нова hxlibc, справжній Wayland у Nook, hxdm замість LightDM) · Замінює частини §5 (Hamix ABI) і §7 (hxproto) плану B,
що стосуються нативних викликів. Порядок робіт -- §5.

## 1. Діагноз (стан на 2026-09-29)

* Нативні програми формально лінкуються з hxlibc (`libhxc.so`, ADR-0015B), але
  `hxstd` усе ще ходить повз libc: 100+ викликів `SYS_HAMIX_*` (9000+) для процесів
  (`SPAWN`, `WAITPID`, `KILL`, `PROC_LIST`), IPC (`MSG_SEND/RECV`, `SERVICE_*`,
  `MAILBOX_FD`, `SHM_*`), терміналів, дисків, мережі, екрана, звуку, GPU, живлення.
  Такі програми неможливо зібрати на Linux чи BSD: hxserver, Nook і hxproto прибиті до
  ядра HamixOS.
* Ядро: усі нативні виклики -- один файл `kernel/src/syscall/native/mod.rs`
  (1280 рядків), а загальні POSIX-виклики живуть частково в `syscall/mod.rs`, частково
  в `syscall/linux/*` -- дерево каже "це Linux", хоча це ядро POSIX для всіх.
* Дві семантики процесу: legacy (`Abi::Native`: неявна консоль на fd 0-2, сигнали
  вбивають, окремі `mkdir/unlink/...`) і POSIX (`Abi::Hamix`, `Abi::Linux`).
* Пристроїв у `/dev` мало: `fb0`, `dri/card0` (лише dumb KMS для Linux-шару), `pts`;
  немає `/dev/input/event*`, `/dev/dsp`, `/dev/sd*` для програм.
* `.desktop` нативних програм лежать у спільному `data/overlay`, а не з програмою.
* Бібліотеки HamixOS (`hxproto`, `hxclient`, `vellum`) -- лише Rust-крейти, без
  `.so`, заголовків і `pkg-config`, тож C/C++-програми їх не використають.
* Wayland-програми зі світу Linux (foot, Xwayland, dillo) ще не зібрані нативно.

## 2. Ціль

1. **Одна модель програми.** Кожна програма -- і власна, і портована -- звертається до
   системи лише через hxlibc (POSIX + звичні розширення Linux/BSD: `epoll`, `memfd`,
   `eventfd`, `timerfd`, `signalfd`), файли пристроїв і `ioctl`. Жодних сирих
   системних викликів у програмах; `hxlibc-sys` -- лише FFI до libc.
2. **Переносимість.** `hxstd`, `hxproto`, `hxclient`, `vellum`, hxserver/Nook збираються
   на HamixOS, Linux і FreeBSD без змін коду (різниця -- лише в константах libc).
3. **Інтерфейси як у Linux/BSD:**
   * процеси -- `fork`/`posix_spawn`/`waitpid`/`kill`, список -- `/proc`;
   * IPC -- `AF_UNIX`-сокети (`SOCK_SEQPACKET`) у `$XDG_RUNTIME_DIR`, спільна пам'ять --
     `memfd_create` + `SCM_RIGHTS` (як Wayland);
   * екран -- DRM/KMS `/dev/dri/card0` (dumb buffers, page flip, cursor planes);
   * ввід -- evdev `/dev/input/event*`;
   * звук -- OSS `/dev/dsp` (рідний для FreeBSD, на Linux -- через ALSA-адаптер у hxsound);
   * диски -- `/dev/sd*`, `/dev/sr*` + `/sys/block`, монтування -- `mount(2)`;
   * живлення -- `reboot(2)`; мережа -- сокети + `ioctl SIOC*`/netlink, Wi-Fi --
     демон `hx-netd` через сокет;
   * вхід/паролі -- `hx-authd` через сокет, `/etc/shadow`, `crypt(3)`.
4. **Ядро:** POSIX-ядро (`syscall/posix/*`), Linux-особистість (`syscall/linux/*` --
   тільки справді Linux-специфічне: перекладач шляхів, `generic.rs`, procfs-адаптації),
   мінімум Hamix-розширень (`syscall/hamix/*`, по файлу на підсистему) -- лише те, для
   чого в Unix немає файла пристрою. Legacy-семантика (`Abi::Native`) зникає.
5. **Бібліотеки як у Linux:** `libhxproto.so` + `hamix/hxproto.h` + `hxproto.pc`
   (C ABI поверх Rust-реалізації), так само `libhxclient`; Rust-крейти -- обгортки.
6. **Програми як у Linux:** кожна програма несе свій `.desktop`, іконки і дані у своєму
   каталозі (`userland/<роль>/<назва>/data/`), xtask ставить їх у стандартні місця.
7. **Wayland-екосистема:** нативні foot, Xwayland (з `xkbcomp`, шрифтами X), dillo;
   нативні Wayland-програми під'єднуються до hxwayland так само, як Linux-програми.

## 3. Що НЕ робимо

* Не переписуємо Nook: API `hxstd::sys`/`hxclient` лишається, змінюється реалізація.
* Не ламаємо Linux-шар: Linux-особистість і `/linux` працюють як зараз.
* Не видаляємо виклик, доки хоча б одна програма в дереві ним користується: спершу
  переносимо всіх користувачів, потім позначаємо `removed` у `hamix-abi.md`.

## 4. Етапи

### U0. Порядок у дереві (цей тиждень)
- [x] План (цей файл).
- [x] `kernel/src/syscall/native/mod.rs` -> файли за підсистемами (`process`, `messages`,
      `term`, `files`, `disk`, `commands`, `auth`, `net`, `display`, `sound`, `gpu`,
      `power`, `debug`); `mod.rs` лише збирає диспетчери. `cargo xtask abi` читає весь каталог.
- [x] `.desktop` кожної нативної програми -- у її каталозі
      (`userland/<роль>/<назва>/share/applications/org.hamix.<Name>.desktop`); xtask копіює
      `share/` крейта в `/usr/share`, як `make install`. У `data/overlay` їх більше немає.

### U1. hxstd поверх POSIX
- [x] Файли, каталоги, права, час, ідентичність, сон, pipe, fcntl, `isatty`, `TIOCGWINSZ`,
      `waitpid`, `kill`, `exit`: `userland/libs/hxstd/src/sys/posix.rs` -- лише функції libc.
      Старі сирі виклики -- `sys/legacy.rs`, лише для aarch64/riscv64, поки там немає hxlibc.
- [x] Решта нативних викликів ідуть через `__do_syscall6` із `libhxc.so` (hxlibc-sys),
      тобто теж через libc; `syscall`-інструкцій у програмах на hxlibc більше немає.
- [~] hxlibc для aarch64/riscv64: збирається (`cargo xtask hxlibc-build --arch A`; builtins з Alpine
      `compiler-rt`, обгортка `hxlibc/cross/bin/<arch>-unknown-hamix-gcc` -> `ld.lld`, riscv64 --
      `rv64gc/lp64d` і лінкування через драйвер Linux; патчі 0021 (база ld.hx через `__ehdr_start`),
      0022 (`R_TLSDESC` у `.rela.dyn`); `rt_sigreturn` = 15, справжні `crti/crtn` для riscv64).
      Rust-програми портів лінкуються з `ld.hx` (`target/specs/<arch>-hamix.json`); обидва порти
      завантажуються до `login:`, hxsvc/hxlogin/служби працюють. Відкладено: bash на портах падає
      (пошкодження пам'яті), `spike` показує хибний `st_size` після `stat` і зависання на
      `pthread_create`; швидке введення з UART губиться. Після виправлення -> видалити `legacy.rs`
      і `Abi::Native`.
- [x] Процеси: `spawn*` -> `posix_spawn` (власна група процесів, `adddup2` для fd 0-3,
      `addclosefrom_np(3)`, фонові діти прибираються `waitpid(WNOHANG)`). `SPAWN_FOREGROUND` =
      `posix_spawn` + `tcsetpgrp`; `SPAWN_ROOT` = `setuid(0)` у setuid-програмі.
- [x] setuid як у Unix: ядро враховує біт `S_ISUID` в `execve` (euid = власник файла,
      `AT_SECURE`), xtask ставить `4755` програмам із `setuid = [...]` у `[package.metadata.hamix]`
      (sudo, su). hx-authd лише перевіряє пароль (ідентичність -- реальний uid з `/proc/<pid>/status`),
      квитки ядра (`GRANT_TICKET`, `root_ticket_until`) більше не потрібні; GUI-підвищення
      (`hxclient::ui::elevated_spawn`) = `sudo -S`. Тест `native-disk-posix`.
- [x] `proc_list`/`sysinfo`/`cpu_stats` -> `/proc/<pid>/{stat,status}`, `/proc/meminfo`,
      `/proc/stat`, `/proc/cpuinfo`. Один `/proc` для всіх: формат Linux + рядки HamixOS
      (`Abi:`, `KernelHeap:`, `FileCache:`...), `tty_nr` = `/dev/ttyN`. Тест `native-procfs`.
- [x] Термінал: `tcsetpgrp` замість `SET_FOREGROUND`, `TIOCSWINSZ` замість `SET_TERMINAL`;
      hxterm -- лише `posix_openpt` (без запасного шляху через pipe), читання неблокувальне
      замість `st_size` pty. `read_key`/`poll_key` читають fd 0 у сирому режимі termios
      (ESC-послідовності -> `Key`), тож hxlogin/hed працюють поверх tty.
- [x] Перевірка: `sys/posix.rs` не містить жодного `SYS_HAMIX_*`/`syscallN`.
- [ ] Лишились нативними в hxstd: `display.rs` (зміна режиму, апаратний курсор, прискорення GPU),
      `net.rs` статус/Wi-Fi/налаштування (-> `hx-netd`), `console.rs` (`TERMGEN`), `futex_*`,
      `abi_info`, `session_attach`, `linux_control`, `trace_syscalls`, `users_reload`,
      `fbmap`/`mouse` (запасний шлях hxserver без DRM/evdev).

### U2. IPC і hxproto на сокетах
- [x] Поштова скринька процесу -- `AF_UNIX`/`SOCK_DGRAM` у `/run/hamix/ipc/<pid>`, відправник --
      з `SCM_CREDENTIALS` (ядро), перед відправкою перевіряється власник сокета.
      `msg_send`/`msg_recv`/`msg_recv_from` (з локальною чергою), `mailbox_fd`, `wait_any`.
- [x] Реєстр служб -- файли з pid: системні `/run/hamix/services` (root), користувацькі
      `$XDG_RUNTIME_DIR/hamix-services`; активація на вимогу -- `/run/hamix/activators/<ім'я>`
      + повідомлення `activate` до hxsvc.
- [x] Буфери вікон -- POSIX `shm_open("/hx-<id>")` + `MAP_SHARED` (у ядрі -- memfd);
      hxserver прибирає буфери мертвих процесів. hxserver, hxclient, hxwayland, hx-authd,
      hxsvc, hxctl працюють поверх цього без змін коду. Опис -- `docs/reference/ipc.md`.
- [x] Повідомлення ядра -- uevent через `NETLINK_KOBJECT_UEVENT`, як у Linux: запит логіну на
      вільному VT = `change@/devices/virtual/tty/ttyN` з `HAMIX_LOGIN=1` (слухає hxsvc),
      hotplug дисплея = `change@/devices/virtual/drm/card0` з `HOTPLUG=1` (слухає hxserver).
      `hxstd::uevent::Monitor`. Старий mailbox ядра hxstd більше не читає.
- [x] `wait_event`/`wait_any` у hxstd -- `poll()` (mailbox-сокет, fd, таймер) + SIGCHLD
      через self-pipe; hxserver чекає на mailbox, `/dev/input/event*` і uevent, hxterm -- на pty.
- [ ] Прибрати `SYS_HAMIX_MSG_*`, `SERVICE_*`, `SHM_*`, `MAILBOX_FD` з ABI (статус `deprecated` уже зараз).

### U3. Пристрої замість викликів
- [x] evdev `/dev/input/event0..2` (`kernel/src/drivers/input/evdev.rs`): клавіатура
      (усі джерела скан-кодів: PS/2, USB, virtio), відносна миша, абсолютний вказівник
      (virtio/USB-планшет); ioctl `EVIOCG*`/`EVIOCGRAB`/`EVIOCSCLOCKID`, `SYN_DROPPED`,
      події лише клієнтам активного VT; клавіатура, відкрита на активному VT, не пише в
      консоль. sysfs для udev (`/sys/class/input`, `/sys/dev/char/13:6x`, `uevent` з
      `EV/KEY/REL/ABS`), `st_rdev`, netlink `NETLINK_KOBJECT_UEVENT`. Порти libudev-zero,
      mtdev, libevdev, libinput; тест `native-evdev` (`libinput list-devices`).
      hxserver читає `/dev/input/event*` через `hxstd::input` (розкладка -- у userspace).
- [ ] `READKEY/POLLKEY/MOUSE` -> removed (лишились для hxlogin/hed/hsh у консолі та для
      aarch64/riscv64, де ще немає hxlibc).
- [x] DRM/KMS для всіх POSIX-програм: `/dev/dri/card0` (dumb buffers XRGB/ARGB/RGB888,
      `PAGE_FLIP` з подіями, `DIRTYFB` з прямокутниками, властивості `DPMS`/`EDID`/`type`,
      вкладений epoll). hxserver показує кадр через DRM (`hxstd::drm`), `FBMAP` -- запасний
      шлях. Апаратний курсор (`CURSOR2`) і зміна режиму через `SETCRTC` -- ще ні.
- [x] OSS `/dev/dsp` + `/dev/mixer` (`kernel/src/drivers/audio/oss.rs`): U8/S16_LE,
      `SPEED/CHANNELS/SETFMT/SETFRAGMENT/GETOSPACE/GETODELAY/SYNC/SETTRIGGER/SETPLAYVOL`,
      мікшер `VOLUME` + `SPEAKER` (вимкнення звуку). `hxstd::audio` і hxsound/hxvideo/Nook
      працюють поверх них; тест `native-audio-oss`. `AUDIO_*` -> removed -- наступний крок.
- [x] Блокові пристрої `/dev/sdX` (`kernel/src/drivers/block_node.rs`): `read`/`write`/`pread`/
      `pwrite`/`lseek` з довільним вирівнюванням, `BLKGETSIZE64`/`BLKSSZGET`/`BLKRRPART`/`BLKFLSBUF`,
      `S_IFBLK` 8:N, лише root, запис поверх змонтованого hext -> `EBUSY`. `/sys/block/<disk>`
      (`size`, `dev`, `removable`, `device/model`, розділи з `start`/`partition`, `uevent` з
      `ID_BUS`, `ID_FS_TYPE/LABEL/UUID`, `ID_PART_ENTRY_TYPE/FLAGS`), `/sys/class/block`,
      `/sys/dev/block`, `/proc/partitions` у форматі Linux. hxstd `disk_*` -- через `/dev` і
      `/sys/block`. `/dev/srX` для CD -- ще ні; `DISK_*` -> removed -- після hsh.
- [x] `mount(2)`/`umount2(2)` для hext (root), псевдо-ФС (`proc`, `sysfs`, `tmpfs`...) --
      без помилки; hxstd `mount`/`umount` -- через libc.
- [x] `reboot(2)` (169, `RB_AUTOBOOT`/`RB_HALT_SYSTEM`/`RB_POWER_OFF`, лише root) замість `POWER`.
- [x] Сокети hxstd (`TcpStream`, `UdpSocket`, `resolve` = `getaddrinfo`, `ping`) -- BSD-сокети libc;
      ядро має ICMP-сокети `SOCK_DGRAM`/`IPPROTO_ICMP` («ping socket» Linux).
- [ ] Статус мережі/Wi-Fi/налаштування -- `hx-netd` + `ioctl SIOC*`/netlink.

### U4. Ядро: POSIX-ядро окремо від Linux-особистості
- [x] `syscall/posix/*` (файли, процеси, сигнали, сокети, tty, mmap, epoll, DRM, netlink, ...);
      `syscall/linux/*` -- лише перекладачі (`generic.rs` -- нумерація asm-generic, `mod.rs` --
      шляхи `/linux`: `guest_path`, `view_of`, `native_of`); `syscall/hamix/*` -- розширення,
      файл на підсистему. `cargo xtask abi` читає `syscall/hamix` і `syscall/posix/mod.rs`.
- [ ] `Abi::Native` (legacy-нота `1`) видалено; `link.ld`-збірка -- лише для aarch64/
      riscv64, доки там немає hxlibc, потім зникає.
- [ ] `hamix-abi.md` v2: core = POSIX, розширення -- короткий список.

### U5. Бібліотеки як у Linux
- [x] Дерево: власний код -- `userland/<роль>/<назва>`, порти -- `ports/<роль>/<назва>` (ті самі
      ролі), сторонні Rust-крейти -- `vendor/`. Бібліотеки Unix і HamixOS лежать на одному рівні
      (`ports/libs/*` і `userland/libs/*`), але окремо. ADR-0017B.
- [x] `libhxproto.so.0` (cdylib `userland/libs/libhxproto`, `no_std`, лише `libhxc.so` у
      `NEEDED`) + `<hamix/hxproto.h>` + `hxproto.pc`: вікно з буфером у спільній пам'яті,
      present, resize, події, заголовок/app-id/курсор/сповіщення. Транспорт -- `hxproto::client`
      (раніше жив у hxtoolkit). xtask: `clib = "назва"` у `[package.metadata.hamix]` ставить
      `.so.N` у `/usr/lib`, а `.so`, заголовки й `.pc` -- у корінь розробки. Тест
      `native-libhxproto` (C, `pkg-config hxproto`).
- [ ] `libhxclient.so` (віджети vellum для C) -- або замість нього C API hxtoolkit.
- [ ] Збірка hxserver/Nook на Linux (host) як перевірка переносимості (CI-крок).

### U6. Wayland/X-екосистема нативно
- [x] Enlightenment 0.26 (E26) як Wayland-компоситор на DRM + libinput:
      `ports/desktop/enlightenment` (EFL 1.27 у `ports/libs/efl`, хост-інструменти
      EFL збираються в `target/hx-ports/ARCH/efl-host-1.27.0`). Запуск -- `startx
      enlightenment` або `session desktop enlightenment`; обгортка
      `/usr/bin/enlightenment-session` (XDG_RUNTIME_DIR, D-Bus, `E_WL_FORCE=drm`).
      Мікшер E -- власний бекенд OSS. Тест `native-enlightenment`.
- [x] foot 1.19 (fcft, tllist) -- `ports/apps/foot`, тест `native-foot`.
- [x] Xwayland 24.1.4 + xorgproto, xcb-proto, libxcb, libXau, libXdmcp, xtrans, libX11,
      libXext, libXrender, libXfixes, libXcursor, libXft, libXinerama, libxkbfile, xkbcomp,
      libfontenc, libXfont2, libxcvt, libxshmfence (шрифти -- `built-ins`); hxwayland
      бере `/usr/bin/Xwayland`, а `/linux/usr/bin/Xwayland` -- лише запасний. Тест `native-x11`.
- [x] dillo 3.2.0 на FLTK 1.3.9 (X11 через Xwayland), `ports/apps/dillo`.
- [x] Ядро: TCP через 127.0.0.1 (і власну адресу) -- пари сокетів у ядрі, без мережевої
      карти (раніше `ECONNREFUSED`; потрібно dpid у dillo). Тест `tcp-loopback`.

### U7. hxtoolkit -- кросплатформна бібліотека інтерфейсу для програм Nook
Зараз програми Nook (hxcalc, hxfiles, hxnotes, hxterm, hxview, hxvideo, hxinstall,
hxsettings, hxmon...) спілкуються з hxserver дуже низькорівнево (hxproto/hxclient,
ручне малювання й обробка подій), тому писати їх важко, а поза Nook вони не працюють.
- [x] Крейт `crates/hxtoolkit` (окремий cargo-крейт, переїде в свій репозиторій): ядро
      `no_std + alloc` -- дерево елементів (`text`, `button`, `input`, `editor`, `checkbox`, `switch`,
      `slider`, `progress`, `dropdown`, `list`, `column!`/`row!`, `scroll`, `image`, `canvas`),
      рушій розкладки, `App` у стилі Elm (`view`/`update`/`key`/`menu`/`tick`), `Context`
      (таймери, буфер обміну, файлові діалоги, повідомлення/підтвердження, сповіщення).
      Бекенд `nook`: сам говорить з hxserver протоколом hxproto через AF_UNIX-сокет і `shm_open`
      (POSIX-функції libc, без hxstd/hxclient), малює у стилі Nook власним кодом (vellum).
      Бекенд `gtk`: gtk4-rs 0.10, узгодження дерева віджетів на місці. Редактор з виділенням,
      undo/redo, номерами рядків, пошуком (`cx.edit`/`cx.find`), меню з підменю (рядок меню в
      Nook), `set_wallpaper`. Опис -- `crates/hxtoolkit/README.md`. Переносні системні
      дрібниці програм (файли, каталоги, процеси з виводом, `sudo -S`) -- крейт `hxport`
      (hxstd на HamixOS, std деінде). Тести `native-toolkit`, `native-toolkit-drive`,
      `native-disk-toolkit` (скріншот -- `HX_QEMU_MONITOR=<sock> cargo xtask test`).
- [ ] Rust-бібліотека **hxtoolkit**: окремий крейт, який згодом буде опубліковано на
      crates.io (репозиторій на GitHub). Поки що живе в цьому дереві як самостійний крейт
      (без залежностей від решти HamixOS, крім опційного бекенда hxserver), щоб її було
      легко перенести. Орієнтовний розмір -- ~5 тис. рядків і більше.
- [x] Бекенд обирається під час виконання: сокет hxserver -> власний бекенд Nook;
      сокет Wayland (`WAYLAND_DISPLAY`) -> бекенд GTK, щоб програма виглядала рідною в
      будь-якому DE (Enlightenment, GNOME, KDE...). Програма про бекенд нічого не знає.
      Прапорець програми: `Settings::hxserver_only()` / `wayland_only()` /
      `display(DisplayServer::Auto)` (типово -- автоматичний пошук); змінна
      `HXTOOLKIT_DISPLAY` звужує `Auto`. Тест `native-files-props` перевіряє відмову
      `wayland` на збірці лише з Nook.
- [ ] GTK та інші складні речі -- через готові крейти (gtk-rs тощо), щоб сама
      hxtoolkit лишалась невеликою; бекенд hxserver -- поверх hxclient/vellum.
- [ ] API має бути дуже зручним: декларативні вікна й віджети (кнопки, поля, списки,
      меню, діалоги, панелі), розкладки, прив'язка стану, таймери, файлові діалоги,
      буфер обміну; однаковий код для обох бекендів.
- [ ] Перевести на hxtoolkit усі програми Nook; після цього вони збираються й
      працюють і на Linux/BSD (Wayland + GTK). Готово: hxhello, hxcalc, hxmon, hxnotes, hxcmd,
      hxview, hxinstall, hxfiles (збираються і для HamixOS, і для Linux/GTK); hxsettings, hxterm і
      hxvideo -- лише для HamixOS (працюють з пристроями й ОС напряму). Усі програми Nook тепер на
      hxtoolkit; hxclient лишився тільки в hxserver і тестових утилітах.
- [x] Спливні меню, контекстні меню й випадні списки -- окремі popup-вікна (`CreatePopup`), як у
      Linux-програмах; `cx.show_menu`, `list().on_context`, `list().grid()`, `App::watch()` (fd),
      `App::appearance()`. Вибір файлу: hxfiles `--pick` друкує шлях у stdout (як zenity),
      hxserver читає його з каналу.
- [x] Nook: вікна з'являються по центру робочої області (діалоги -- над своїм вікном), закриття
      кнопкою в програмі анімується, кнопка закриття спершу питає програму (Wayland-модель) і
      примусово закриває після повторного натискання через 3 с; меню користувача в панелі.
- [x] `set_wallpaper` -- запит `SetWallpaper` до hxserver; Nook показує системний діалог з
      переглядом шпалер і кнопками «Set wallpaper»/«Cancel», відповідь -- `WallpaperResult`.
      `Reload`/`DisplayChanged` прибрано з hxproto: hxserver перечитує nook.conf і режим екрана
      за `SIGHUP`, як звичайний демон.
- [ ] Приклади й документація крейта; тести на обох бекендах (Nook і hxwayland/E).
- [x] Files: «Properties» -- окреме вікно-процес (`hxfiles --properties=ШЛЯХ`) з повною
      інформацією і зміною прав; Help -> «About Files» (`hxfiles --about`). Мультивіконність
      у hxtoolkit поки що -- через окремі процеси (кожен процес -- одне з'єднання з hxserver).

### U7a. Дозволи програм через групи Unix
- [x] Групи як в Android/BSD: `audio`(29), `video`(44), `input`(97), `inet`(3003, AID_INET),
      `power`(3004), `sysconf`(3005). Ядро: `egid`/`rgid`/`groups`, `setgroups`/`getgroups`,
      `chown` з групою, права за групою, `S_ISGID`, `access(2)` за реальними id. hxlibc
      `initgroups` (патч 0023); hxlogin і `hxsvc --as` викликають `initgroups`.
- [x] `socket(AF_INET)` лише для root і членів `inet`; пристрої -- через `/etc/devfs.conf`
      (FreeBSD, застосовує hxsvc), власники й режими файлів образу -- через mtree-специфікацію
      `/etc/mtree/hamix.dist` (застосовує mkhext).
- [x] useradd/usermod/userdel ведуть членство в `/etc/group`; нові користувачі -- у
      audio/video/input/inet, адміністратори (`-G sudo`) ще й у power і sysconf.
- [x] `/usr/libexec/hamix/hxperm-exec APP-ID PROGRAM` (setuid root, як `sg`): прибирає групи
      дозволів, яких програмі не дано, і запускає її від користувача. Програма оголошує
      дозволи в `.desktop` (`X-Hamix-Permissions`, `X-Hamix-Permissions-Required`); рішення
      користувача -- `/etc/hamix/permissions/<APP-ID>`. Чужі .desktop без оголошення отримують
      inet/audio/video/input. Дозволи лише забирають права облікового запису, нових не дають.
      Nook запускає всі програми через hxperm-exec, startx/hxlogin -- сам hxserver
      (`org.hamix.Nook`).
- [x] Супердозволи: `poweroff`/`reboot`/`halt` -- `root:power 4750` (модель BSD operator), тож
      Nook вимикає й перезавантажує ПК без пароля, якщо має дозвіл `power`; без нього --
      діалог з паролем. `hxsysconf` -- `root:sysconf 4750`: `permit`, `reset`, `show`,
      `session`, `linux`, `netconf`, `hostname`, обмежений `setconf`. Налаштування користуються
      ним без пароля, якщо мають дозвіл `sysconf`, інакше -- `sudo -S hxsysconf`.
- [x] Нові Налаштування: сторінка «Apps & permissions» (вибір програми, перемикачі дозволів і
      супердозволів, скидання), Appearance, Effects, Display, Network, Sound, Account, Startup,
      System status, About. Тести `native-permissions`, `native-disk-install`, `native-disk-posix`.
- [ ] Окремі дозволи для камери/Bluetooth/розташування, коли з'являться пристрої;
      дозволи на рівні користувача (зараз `/etc/hamix/permissions` спільний для системи).

### U8. Збірка системи з вибором DE: Nook або Enlightenment
- [x] `cargo xtask build --desktop all|nook|enlightenment` (типово `all`). Програми Nook позначені
      `desktop = "nook"` у `[package.metadata.hamix]`; у варіанті Enlightenment їх, `/usr/share/nook`
      і шпалер Nook немає, сесія за замовчуванням -- `enlightenment-session`. У варіанті Nook
      пакунки E/EFL і те, що потрібне лише їм (libinput, libevdev, mtdev, dbus, util-linux, lua51...),
      прибираються за списками файлів hx-ports. `/etc/hamix/desktop` каже, що в образі;
      `cargo xtask test` пропускає тести іншого DE.
- [ ] Hamix Install у варіанті Enlightenment: потрібен порт GTK 4 для бекенда hxtoolkit `gtk`
      (зараз є лише GTK 3); поки що там лише `install.sh` з термінала.
- [x] Тести й `membench` для обох варіантів (`out/bench/membench-<desktop>.json`), 2026-10-05:

      | варіант | ISO | пам'ять: консоль | пам'ять: DE без програм |
      |---|---|---|---|
      | nook | 281 MiB | 19 MiB | 39 MiB |
      | enlightenment | 323 MiB | 19 MiB | 103 MiB |
      | all | 358 MiB | 19 MiB | 39 MiB (Nook) |

### U9. Оптимізація Enlightenment
Зараз E працює повільно і місцями через обхідні шляхи.
- [ ] Спільний page cache у ядрі, щоб `mmap(MAP_SHARED)` звичайних файлів був
      когерентним між процесами; прибрати патчі EFL/E, що переводять `wl_shm` і
      keymap на `memfd_create`.
- [x] DRM: кадр копіюється прямо зі сторінок dumb-буфера у framebuffer (раніше -- через тимчасовий
      рядок і `shm_io` з глобальним блокуванням на кожен рядок).
- [x] evas малював у RGB888: EFL записувала бажану *глибину* 24 як *bpp* (патч EFL 0003 -- глибина 24
      = 32 біти на піксель, XRGB8888), тож без поштучного перетворення пікселів.
- [ ] `PAGE_FLIP` без копіювання (scanout прямо з dumb-буфера, де дозволяє драйвер), апаратний
      курсор (`CURSOR2`).
- [x] Wayland-програми в E: E оголошував `wl_seat` v4, а foot і сучасні клієнти потребують v5 --
      патч E 0008 (seat v5 + `wl_pointer.frame`). Тест `native-enlightenment-apps` запускає foot,
      GTK 3, hxcalc, hxfiles і dillo з оточенням E (через нове `/proc/<pid>/environ`).
- [x] Програми Nook (hxtoolkit) у будь-якому Wayland-композиторі: власний Wayland-транспорт
      (`wl_shm` + `xdg_shell` + `wl_seat`, клавіатура evdev → коди toolkit, автоповтор) поруч із
      транспортом hxserver; бекенд `Backend::Wayland` без GTK.
- [ ] X11-програми в E (dillo): EFL зібрано без `ecore_x`, тож E не запускає Xwayland -- зібрати EFL з
      X11 або запускати Xwayland з окремим XWM.
- [x] Точний `CLOCK_MONOTONIC`: на x86_64 -- TSC, відкалібрований за PIT при старті (aarch64/riscv64 --
      апаратні лічильники); `clock_gettime`, `gettimeofday`, абсолютні timerfd і `clock_nanosleep`
      користуються одним годинником (`task::clock_ms`), `clock_getres` = 1 нс.
- [ ] vblank за реальним дисплеєм; мітки часу evdev у нс.
- [ ] Профілювання E (`cargo xtask prof`): основний цикл, evas software engine,
      epoll/timerfd; прибрати причини частих пробуджень.
- [ ] Системна шина D-Bus і upower/login1 для E (зараз помилки `system_bus_socket`),
      щоб модулі не падали в запасні шляхи; сесійна шина без `dbus-run-session`.
- [ ] Прискорення рендерингу: evas через Mesa (GL) або принаймні SIMD-оптимізований
      software engine (`native-arch-optimization`).
- [ ] Прибрати інші обхідні рішення: `ptrace` -> `EPERM`, заглушка `inotify` без
      подій (справжні події з VFS), ручні обгортки сесії.

### U10. Стабільність, безпека і справжнє залізо (уроки Redox OS)
Скарга (2026-10-05): на справжньому залізі не вантажаться драйвери й не створюється віртуальна
ФС -- система зависає, хоча у QEMU все гаразд; часті kernel panic; вікна (термінал та інші)
мерехтять; система заплутана й небезпечна.

Як це розв'язує Redox OS: драйвери -- звичайні процеси в просторі користувача з обмеженим
простором імен (падіння драйвера не валить ядро); `pcid-spawner` запускає драйвер за правилами
`/etc/pcid.d/*.toml` (клас/vendor/device); переривання драйвер читає з `irq:`-схеми і сам
підтверджує; маленький `initfs` (ramdisk) містить лише те, що потрібно, щоб дістатися кореня
(ahcid, nvmed, usb-накопичувачі, redoxfs) і `init.rc`; решта драйверів -- після монтування кореня;
`fbbootlogd` показує журнал завантаження на екрані; libc -- relibc на Rust; ядро мінімальне.

Зроблено 2026-10-05:
- [x] Кожен крок hxinit друкує `[ .... ] Назва` *до* запуску, а модулі -- `starting <ім'я>`, тож
      на екрані видно, де саме зависло (серійний порт: `hxinit: start/progress/slow`).
- [x] Сторож завантаження: крок, що триває > 10 с, показує `[ SLOW ]`, назву драйвера й підказку
      (`blacklist=<модуль>` або пункт меню «safe graphics, no driver modules»).
- [x] Захист від шторму переривань (як «irq N: nobody cared» у Linux): спільна рівнева лінія, яку
      ніхто не обслуговує 20 000 разів поспіль, маскується, драйвери на ній переходять на опитування;
      повідомлення в dmesg. На реальних ноутбуках лінії спільні (USB + SATA + GPU на одній), у QEMU --
      ні; шторм зупиняв таймер і всі `delay_ms` драйверів.
- [x] Купа ядра: подвійне звільнення, перезаписаний заголовок чи зіпсоване посилання вільного
      списку більше не дають випадкового `#GP` пізніше -- блок карантинується (втрачається), подія
      пишеться в серійний журнал, лічильник -- `HeapCorrupt:` у `/proc/meminfo`.
- [x] Мерехтіння вікон Nook: hxtoolkit малював прямо в спільний з hxserver буфер (спершу заливка
      тлом, потім віджети), і композитор показував напівготовий кадр. Тепер кадр малюється в
      приватний задній буфер і копіюється одним `memcpy` перед `Present`.
- [x] Тест `native-stress-desktop` (6 програм × 6 раундів + навантаження оболонки), проходить з
      `--smp 1` і `--smp 4`.

Далі:
- [x] Тести за замовчуванням як ноутбук: `-smp 2`, `-m 3G` (усі 39 проходять; модулі вантажаться
      біля 3 ГіБ, як на залізі).
- [ ] Окремі варіанти тестів із завантаженням з USB-флешки (xHCI + usb-storage) і з UEFI.
- [x] `hwreport [ФАЙЛ]` (скрипт `/usr/bin/hwreport`): `/proc/hxinit`, командний рядок, CPU, пам'ять,
      PCI з IRQ, драйвери, модулі, переривання, USB, графіка, звук, диски, `dmesg` -- в один файл і
      підказка, як скопіювати на флешку. Тест `native-hwreport`.
- [x] Екран паніки: причина вгорі, останні рядки журналу ядра (без очікування блокування),
      прохання сфотографувати екран і надіслати разом із hwreport. `hxpanic=boot` -- навмисна паніка
      для перевірки екрана.
- [x] IOAPIC (`arch/x86_64/ioapic.rs`): IOAPIC беруться з MADT, PCI-лінія > 15 (так її пишуть BIOS
      у режимі APIC) маршрутизується на вільний вектор як рівнева active-low, спільний GSI ділить
      вектор, шторм на GSI маскує пін в IOAPIC. Раніше такий пристрій узагалі не мав переривань
      (`-19`). 0..15 лишаються на 8259 (virtual wire), MSI як і раніше має пріоритет. Крок `acpi`
      показує кількість IOAPIC і GSI.
- [x] `initfs` як у Redox: `/boot/initfs.tar` містить лише драйвери з `early = 1` (зараз `ahci`) і
      `etc/initfs.conf`, а не весь `lib/modules`; GRUB: `module2 /boot/initfs.tar initfs`, ядро
      приймає й старе `kmods` (встановлені системи). Крок hxinit -- «Boot drivers (initfs)».
- [x] Передача контролера: якщо корінь змонтовано через вбудований драйвер (немає initfs або
      ранній модуль не завантажився), автозавантаження модулів більше не забирає той самий PCI-пристрій
      (`pci::claim_builtin`); раніше модуль `ahci` скидав контролер під змонтованим живим коренем і
      читання файлів повертало сміття (`elf: field out of bounds`). Тести `native-noinitfs`
      (модулі без initfs) і `native-nomodules` (безпечний режим) -- скрипти з такими назвами xtask
      завантажує з відповідними параметрами.
- [ ] Драйвери з initfs і решта -- кожен у власному потоці ядра з тайм-аутом і пропуском (зараз
      сторож лише показує `[ SLOW ]`, бо потоки ядра не витісняються); повністю -- лише драйвери в
      просторі користувача (нижче).
- [ ] `_PRT` з ACPI (інтерпретатор AML), щоб маршрутизувати INTx і там, де BIOS не записав лінію.
- [ ] Драйвери в просторі користувача: демон `hxpcid` читає `/etc/hamix/pci.d/*.conf`, запускає
      драйвер-процес; доступ до BAR -- `mmap` `/dev/pci/<slot>/resourceN`, переривання -- читання
      `/dev/irq/<N>` (+ запис для підтвердження), DMA -- фізично суцільний `memfd`. Перші кандидати --
      мережа (yukon, ar9285, e1000e) і звук; падіння такого драйвера перезапускає лише його.
- [~] Ядро не панікує через програми: помилки на шляху системного виклику -- `SIGSEGV`/`EFAULT`
      процесу, не `panic!`. Фазер `tests/hxlibc/syscall-fuzz.c` (тест `native-syscall-fuzz`: діти від
      uid 65534 і 1000, випадкові виклики 0..449 і 9000..9191, вказівники в арену/ядро/неканонічні,
      спільна сторінка з останнім викликом; `HX_TEST_ARGS="fuzz=N,виклики,seed,trace"`). Виправлено:
      неканонічний RIP із `rt_sigreturn`/обробника (#GP на `iretq` у ядрі), `munmap` з переповненням,
      виділення пам'яті ядра розміром від програми (`mremap`, `mincore`, приватний `mmap` memfd),
      `MAP_SHARED|MAP_ANONYMOUS` після `fork`, неперервні сигналами `SLEEP`/`WAIT_EVENT`/`MSG_RECV`.
      Паніка друкує `[syscall N(args) tid T]`. Буфери розміру від програми (`read`/`write`/`pread`, pipe,
      memfd, `/dev/sdX`, сокети, `mmap` memfd, `DISK_*`, `NET_SOCKET_*`) -- `memory::try_zeroed`/`try_copy`:
      спершу `memory::reclaim()` (кеш файлів, кеш читання блоків, `heap::trim`), потім `ENOMEM` замість
      паніки. Далі: решта виділень ядра (структури, а не буфери), фазер довше й з кількома потоками.
- [x] Збірка на OpenBSD (і будь-якому хості без rustup): `cargo xtask kernel` працює stable rustc із
      `RUSTC_BOOTSTRAP=1`; `rust-src` тієї ж версії xtask сам завантажує в `target/rust-src` для `build-std`,
      `rust-lld` -- shim поверх системного `ld.lld` (на OpenBSD -- `-no-pie --no-execute-only`), аліас --
      `--target host-tuple`, GNU-утиліти -- `gcp`/`gtar`/`gsha256sum`, якщо є. `tools/openbsd/tune-build.sh`
      (root): клас входу `hamixbuild` без ліміту пам'яті, `hw.smt=1`, `kern.pool_debug=0`, `noatime` для
      `/home`, пакети. Повний образ на OpenBSD -- ще ні: немає `grub-mkrescue`, порти потребують GNU-оточення.
- [x] `cargo xtask lint-unsafe`: `unsafe` за підсистемами проти `tools/xtask/unsafe-baseline.txt`
      (храповик: більше, ніж у базі, -- помилка; після обґрунтованого ревʼю -- `--update`). База
      2026-10-05: 1208, найбільше -- hxstd (327), kernel/arch (229), kernel/memory (114).
- [ ] Зменшити `unsafe` у hxstd (перехід на hxlibc-sys/libc завершити), `unsafe_op_in_unsafe_fn = deny`
      для ядра.
- [ ] hxproto: два буфери на вікно (`Attach` по черзі) і пошкоджені прямокутники, як `wl_surface`
      commit -- без копії кадру; hxserver композитить лише закомічений буфер.
- [x] Дерево, як у Redox (ADR-0018B):
      * ядро: `syscall/` -- лише POSIX-шар системних викликів (`table.rs` -- номери й диспетчер,
        `base`, `process`, `signal` + `sigframe/`, `misc`, `special`; `ext/` -- розширення HamixOS);
        Linux-особистість цілком у `linux/` (`syscalls.rs`, `paths.rs`, `loader.rs`); підсистеми,
        що жили в `syscall/posix`, -- на своїх місцях: `fs/procfs.rs`, `fs/epoll.rs`,
        `net/{socket,socket_inet,netlink}.rs`, `drivers/video/drm.rs`, `tty/{mod,vt,pty}.rs`;
      * `crates/` -- переносні Rust-бібліотеки без жодних викликів ОС (hext, hxvt, wlan, hxproto,
        hxtoolkit, vellum, mini_png, кодеки), як окремі крейти Redox (orbclient, redoxfs);
      * `userland/libs/` -- лише бібліотеки, що говорять з HamixOS (hxstd ≈ libredox, hxport,
        hxclient, libhxproto); верхнього `libs/` більше немає;
      * `hxlibc/` -- повне дерево mlibc з `sysdeps/hamix` (≈ relibc), без завантаження й патчів під
        час збирання; `hxlibc/rust/hxlibc-sys` -- Rust-бік libc, сюди ж піде Rust-код бібліотеки.
- [x] Захист пам'яті (W^X), як у Linux/Redox: раніше `mmap` ігнорував `prot`, `mprotect` нічого не
      робив, а всі сторінки користувача були RWX. Тепер `prot` діє (PTE: `USER`/`WRITABLE`/`NX`, для
      лінивих сторінок -- у вільних бітах), `mprotect` змінює наявні й ліниві сторінки (спільна після
      `fork` сторінка стає COW), ELF-сегменти отримують свої `R/W/X` (сторінка на межі -- об'єднання),
      стек без виконання, якщо `PT_GNU_STACK` не просить інакше. Лише x86_64; aarch64/riscv64 -- поки
      RWX (той самий API). Тест `native-usercopy`: SIGSEGV на запис у `PROT_READ`, NX, W^X.
- [x] `usercopy` (як у Redox): `copy_out` копіює підпрограмою з виправленням збою -- якщо сторінка
      зникла під час копіювання (гонка `munmap` у SMP) або доступна лише для читання, виклик повертає
      `EFAULT`, а не вбиває процес, що тримає блокування ядра. Обробник `#PF` тепер отримує `&mut`
      кадр (запис через `&` -- UB, компілятор його викидав).
- [x] Шари ядра: один `errno.rs` замість 113 розкиданих констант у 17 файлах; `usercopy.rs`;
      таблиця дескрипторів -- `task/fdtable.rs`; `fs/`, `net/`, `tty/` більше не залежать від
      `syscall` (крім сигналів).
- [x] Справжнє залізо (Samsung R428, фото 2026-10-05): корінь не змонтувався, бо GRUB не передав
      `hext.img` (323 МіБ через BIOS із флешки) -- відтворено в QEMU (USB, 2 ГіБ, нестабільно). Тепер:
      крок `bootinfo` показує, що передав завантажувач; без кореня -- зрозуміле пояснення причини й що
      робити; GRUB перевіряє `module2` і повторює спробу з повідомленням. Журнал ядра -- 1024 рядки.
- [x] **USB-накопичувачі в ядрі**: bulk-передачі UHCI (компаньйони EHCI, як на ICH9/R428),
      Bulk-Only Transport, SCSI `INQUIRY`/`TEST UNIT READY`/`READ CAPACITY`/`READ(10)`/`WRITE(10)`,
      блоки 512/2048/4096 байт, гаряче від'єднання (`Backend::Usb`). Тест `native-usb-storage`.
- [x] Знімні носії: `fs/media.rs` -- ISO 9660 (+ Joliet) і FAT12/16/32 (+ довгі імена VFAT) лише
      для читання, дані файлів -- на вимогу; `mount -t auto|iso9660|vfat`, `/proc/mounts` з типом і міткою.
      `/dev/srN` відкриваються як блокові пристрої (major 11). `hxmount` (setuid, як pmount):
      `list|mount|unmount|watch` лише для знімних дисків у `/media/<мітка>`; служба `hx-automount`
      монтує диски й флешки автоматично, а відмонтоване вручну не повертає до від'єднання. Files:
      носії в боковій панелі з іконкою диска/флешки й маленькою кнопкою відмонтування. Тести
      `native-media-mount`, `native-media-usb-automount`.
- [x] EHCI (USB 2.0) для накопичувачів: `drivers/usb/ehci.rs` (асинхронний розклад, BOT, CONFIGFLAG=1,
      не-HS і не-накопичувачі -> компаньйони UHCI), блоковий кеш вікнами по 128 КіБ для USB/DVD, великий
      кеш файлів для живого кореня на повільному носії. Скарга з R428 (2026-10-06): живий образ із
      флешки через UHCI «висне, гуде вентилятор». Тест `native-usbroot` перевіряє high speed і передачу
      планшета компаньйону.
- [~] USB-хаби в EHCI (2026-10-10): вбудований rate-matching hub Intel 6/7 серії (Sandy/Ivy Bridge, Pentium G640),
      за яким висять усі роз'єми і немає компаньйонів UHCI -- живлення й опитування портів хаба, скидання,
      нумерація, від'єднання; full/low-speed пристрої за high-speed хабом -- split-транзакції; передача
      EHCI від BIOS (USBLEGSUP). Скарга: флешка на ПК з G640 не знаходилась ядром, на R428 -- так. QEMU не
      емулює high-speed хаб, тож перевірка -- лише на залізі.
- [ ] Запис на FAT (зараз лише читання), фізичне виймання диска (ATAPI `START STOP UNIT`), виявлення
      вставленого диска без перезавантаження, xHCI-накопичувачі (USB 3.0), HID через EHCI-хаби
      (переривання, періодичний розклад), I/O диска без утримання великого замка ядра
      (зараз активне очікування USB під BKL зупиняє інші процеси).
- [x] Живий корінь з флешки через новий драйвер: без `hext` від GRUB ядро вмикає USB до кроку
      `rootfs`, чекає флешку до 10 с і читає `/boot/hext.img` з ISO 9660 на ній на вимогу. Пункт меню
      «Live from a USB stick» вантажить лише ядро й initfs. Тест `native-usbroot` (флешка за EHCI з
      компаньйонами UHCI, як на ICH9/R428; завантаження з неї через SeaBIOS). Ventoy (фото з R428,
      2026-10-06): образ шукається і в `.iso` на exFAT (`fs/exfat.rs`; спершу `*hamix*`, далі мітка
      тому `HAMIXOS`/`ISOIMAGE`, решту ISO пропускаємо) та на FAT-розділах; фрагментований файл --
      loop-пристрій (`block::register_mapped_disk`, карта екстентів); `livemedia=usb` ігнорує CD; тест
      `native-ventoy` (xtask `exfat.rs` будує флешку як Ventoy: фрагментований ISO + зайвий ISO). Ще
      ні: NTFS-розділ Ventoy, xHCI-накопичувачі.
- [ ] Повільний ATAPI через IDE (PIO): корінь з DVD у режимі IDE монтується ~19 с -- DMA (bus master).
- [x] Перевірка прав запису в `user_slice_mut` (`AddressSpace::is_writable`): запис ядра в
      `PROT_READ` -> `EFAULT` (тест `native-usercopy`: `uname`, `clock_gettime`).
- [ ] TLB shootdown між ядрами для `mprotect`/`munmap`.
- [ ] Великий замок ядра (BKL) і невитіснювані потоки ядра -- дрібні блокування за підсистемами;
      завислий драйвер не має зупиняти все.
- [x] Облікові записи лише в образі: ядро не створює `/etc/passwd`/`shadow`/`sudoers`, не має бази
      користувачів (лише читає `/etc/passwd` для `HOME`/`SHELL` у `default_env`) і входу -- без init
      показує аварійну консоль. Паролі -- `crypt(3)` SHA-512 (`$6$`) у hxlibc, `/etc/shadow` у форматі
      Linux; старий 32-бітний хеш лише перевіряється. Виклики `AUTH`, `USERS_RELOAD`,
      `SET_OWN_PASSWORD`, `GRANT_TICKET` видалено. Тест `native-passwords`.
- [ ] hxclient дублює hxproto::client/hxtoolkit -- перевести hxserver і прибрати.
- [ ] Спрощення: одна дорога для кожної речі -- прибрати `Abi::Native`, legacy-виклики, hxclient
      (лишився лише в hxserver), дублікати реєстрів; одна карта дерева в README.

### U11. Швидкість, LightDM і Nook за схемою Redox (Orbital)
Скарга (2026-10-06): «система супер повільна, з диска трохи краще, але колись працювала швидше»;
прохання портувати повноцінний LightDM з Nook у списку сеансів і зробити DE «як у Redox».

Як у Redox: **Orbital** -- звичайний процес простору користувача, що поєднує дисплейний сервер, віконний
менеджер і композитор (схема `orbital:`; програма відкриває `orbital:прапорці/x/y/w/h/заголовок`, отримує
fd, малює в буфер, читає події з fd). Усе інше -- **окремі звичайні програми**: `orblogin` (вхід), `launcher`
(панель задач і вибір програм зі списку `/ui/apps`), фон, термінал, файли. Драйвери екрана (`vesad`,
`ihdgd`) і вводу -- теж процеси. **Nook зараз інакший:** програми -- окремі процеси (як у Redox), але
панель, док, меню програм, значки робочого столу, регулятор звуку й шпалери зашиті в `hxserver`.

Зроблено 2026-10-06:
- [x] Профілювання (`kbench`, `native-prof-musl` -- окремі тести під `hxprof`): зомбі-потоки прибирались лише
      в простої, `schedule`/`wake_all` перебирали їх усіх -> прибирання одразу в `finish_switch`; кеш файлів не
      стискається для кореня на диску; упереджене читання 128 КіБ для SATA/IDE/USB/DVD; `debug_println` у COM
      лише з `console=ttyS0`; пробудження `poll/select/epoll` не губляться (номер події -- до перевірки).
- [x] COW-збої й ліниве заповнення сторінок -- під BKL (раніше без нього; безблокові виклики
      `clock_gettime`/`gettimeofday` могли заповнювати сторінки паралельно з іншим потоком).
- [x] LightDM повністю: Xorg (modesetting + libinput), PAM, libgcrypt, lightdm-gtk-greeter; ядро -- VT-ioctl,
      збережені uid, відправник сигналу, `pidfd_open`, права запису при `open`; `hxsvc` -- ключ `vt = N`
      (служба займає VT, після її виходу повертається текстовий вхід), `dm=none`; тема іконок `HamixOS`
      (`tools/icons/greeter-icons.py`). Тести `native-xorg`, `native-gtk-x11`, `native-lightdm`,
      `native-lightdm-login`, `native-saved-ids`, `native-strtod`.

Далі:
- [x] Потоки одного процесу на різних ядрах (раніше всі потоки процесу прив'язувались до одного CPU). Знайдено
      й виправлено: два потоки одночасно отримують збій на одній лінивій сторінці -- другий після BKL бачив
      уже заповнену сторінку й отримував SIGSEGV (тепер -- повтор інструкції); помилка mlibc
      `pthread_rwlock_wrlock` (`rc_count_mask` замість біта очікувачів -> лічильник читачів `0x7fffffff`,
      вічний цикл `futex` -> `EAGAIN`; на ній стоїть система типів GObject). Діагностика: рядок `HamixWait:` у
      `/proc/<pid>/status`, `uip` (файл+зсув і байти коду) у звіті `hxprof`. Тест `native-threads-smp`.
- [x] IPC на кількох ядрах: виміряно -- причина не в BKL, а в пробудженні сплячого ядра (IPI + вихід із
      `hlt` на кожен півобіг). Ядро, що щойно заснуло, 40 мкс опитує чергу (`idle_poll`, лише після
      реальної роботи), а будильник не шле IPI ядру, яке опитує. pipe на 2 ядрах: 27 -> 11-16 мкс
      (на одному -- 10). Дрібні замки для pipe/futex/сокетів -- коли знадобиться далі.
- [x] **Nook за схемою Orbital:** `hxserver` -- лише сервер, WM і композитор (вікна, сповіщення,
      перемикач Alt+Tab, меню заголовка вікна, діалоги входу/шпалер); панель, док, меню програм, значки
      робочого столу, мережа й звук -- окрема програма `nook-shell` (як `launcher` у Redox). Оболонка
      складається з шарів hxproto v2: значки (BOTTOM), панель і док (TOP з виключними зонами), спливні
      вікна й OSD (OVERLAY; шар з клавіатурою перехоплює вказівник, клік поза ним закриває меню). Нове в
      hxproto: `ShellAction` (вихід, перезапуск, вимкнення, «Про систему»), `Toast` зі значком,
      `ToplevelRectangle` (куди згортати вікно -- комірка доку), подія `ToplevelGeometry` (розумне
      ховання доку). Прозорі пікселі верхніх шарів пропускають клік до вікон. vellum `Painter` уміє малювати
      в буфер шару з альфою (premultiplied ARGB) в екранних координатах (`Painter::layer`), тож код панелі
      й доку не переписувався. SIGHUP сервера пересилається оболонці; якщо оболонка впала -- сервер
      перезапускає її (до 3 разів за 30 с). Golden-знімки (`cargo xtask golden`) проходять через LightDM.
- [x] LightDM у живому образі: виправлено зависання ~1 з 5 завантажень (LightDM стартував раніше, ніж
      dbus-daemon створив сокет, і мовчки виходив з кодом 1 -- `lightdm-start` чекає сокет);
      `waitid(P_PIDFD)` (GLib не бачила завершення greeter, сеанс Nook стартував поки Xorg ще тримав
      екран); сеанс на VT 2 -- процес, що бере екран, переходить на активний VT (інакше ввід ішов на
      tty1, а hxsvc запускав текстовий вхід поверх Nook); служба `lightdm` займає tty1 і tty2
      (`vt = 1 2`), текстові консолі -- tty3..tty6.
- [x] Дрібні вади Nook (скарга 2026-10-07): док не ховався під новим вікном (геометрія вікна приходила раніше
      за саме вікно), анімації згортання/відкриття тепер розвертаються з поточного кадру (швидкі клацання не
      «зривають» анімацію, згортання під час відкриття працює), мерехтіння при зміні налаштувань (SIGHUP
      перевідкривав DRM і перебудовував усі шари оболонки), курсор-«І» над полями вводу оболонки й діалогу
      пароля, правий клік виділяє значок, підсвітка кнопок не «залипає» (список гарячих зон очищався кожен
      кадр), сеанс LightDM більше не падає, коли Xorg вітальні ще тримає екран (hxserver чекає до 5 с).
- [x] Плавність: тіні вікон кешуються як маски (не рахуються щокадру), шари оболонки -- плитки 32x32
      (порожні пропускаються, непрозорі копіюються), hxtoolkit надсилає лише змінені рядки вікна, `Pixmap`
      копіюється при записі (без копії 1,2 МБ на кожну перебудову), hxterm малює лише змінені клітинки.
- [x] Бібліотека зображень `imago` замість `mini_png`: PNG (усі типи, Adam7), JPEG (baseline і
      progressive, будь-яка субдискретизація, CMYK, EXIF-орієнтація, масштабоване декодування 1/2..1/8), GIF
      (з анімацією), BMP, ICO/CUR, QOI, TGA, PNM/PAM, SVG (шляхи, фігури, градієнти, обрізання, маски,
      розмиття, CSS). Звірено з Pillow і rsvg-convert; inflate швидший за zlib, JPEG 1080p 122 -> 29 мс.
      Значки програм тепер і з `scalable/*.svg`, шпалери -- будь-який формат.
- [x] **Nook Wayland:** `nook-shell` має дві платформи -- `src/render/hxserver/` (hxproto v2) і
      `src/render/wayland/` (wlr-layer-shell, foreign-toplevel, wl_seat, cursor-shape); інтерфейс спільний.
      Збірка: ознаки `hxserver`, `wayland` або обидві (типово); вибір під час роботи -- `NOOK_PLATFORM`.
      Клієнтська частина Wayland -- спільний крейт `wlclient`. Композитор сам приймає Wayland-клієнтів:
      hxwayland став бібліотекою з `Link` і вбудований у hxserver (ознака `wayland`; окремий процес -- лише
      з `NOOK_WAYLAND=external`). Сеанс «Nook (Wayland)» у LightDM. Тест `native-shell-wayland`.
- [ ] Nook Wayland далі: геометрія вікон для доку (власний протокол), сповіщення в оболонці, буфери
      Wayland-клієнтів без копіювання, XWM без мосту.
- [ ] `hx-login1` на рідній системній шині (зараз LightDM обходить logind -- `use-logind=false`); сеанс
      Enlightenment через LightDM; повернення на VT сеансу після Ctrl+Alt+F3.

## 5. Порядок і перевірка

Порядок: U0 -> U6 (foot, бо дає нативний Wayland-термінал без змін системи) -> U1 ->
U2 -> U3 -> U6 (Xwayland, dillo) -> U4 -> U5 -> U7 (hxtoolkit) -> U8 (вибір DE) -> U9
(оптимізація E; ядерні частини, як page cache, можна раніше). U10 (стабільність і справжнє
залізо) -- найвищий пріоритет, раніше за нові функції. Після кожного кроку:
`cargo xtask test` (full і core), `membench` (Nook < 70 МБ), `lint-layers`, `abi --check`.
Стан кроків відмічається в цьому файлі.
