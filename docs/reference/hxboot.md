# hxboot: завантажувач HamixOS, його меню і `hxbootctl`

Status: current · Updated: 2026-10-10 · План: `docs/roadmap/FOUNDATION_PLAN.md` (§8, B1)

hxboot -- власний завантажувач на Rust (`boot/hxboot`), зараз BIOS: El Torito (DVD), гібридна
флешка (той самий ISO, записаний `dd`) і Ventoy. Для UEFI ISO містить окремий GRUB EFI-завантажувач
із multiboot2-меню. Сам hxboot-efi, запуск встановленої системи через UEFI й пряме UEFI-завантаження
з USB ще розробляються. ISO з обома режимами: `cargo xtask build --loader hxboot`.

## Меню

| Клавіша | Дія |
|---|---|
| ↑ ↓ Home End | вибрати пункт |
| Enter, 1..9 | завантажити |
| e | змінити командний рядок ядра лише для цього завантаження (Enter -- завантажити, Esc -- назад) |
| Shift під час старту | показати меню, навіть якщо `timeout 0` |

Будь-яка клавіша зупиняє зворотний відлік.

## Файл `/boot/hxboot.cfg`

Звичайний текст: його можна правити будь-яким редактором або командою `hxbootctl`. Рядки з `#` --
коментарі. Пункт меню починається з `entry НАЗВА`, усі рядки нижче до наступного `entry` належать
йому.

Загальні параметри:

| Рядок | Значення |
|---|---|
| `title ТЕКСТ` | заголовок меню |
| `timeout СЕКУНДИ` | скільки чекати; `0` -- одразу (меню -- лише з Shift) |
| `default N` | пункт за замовчуванням, рахуючи з 0 |
| `probe yes\|no` | шукати інші ОС на жорстких дисках під час кожного завантаження |

Рядки пункту:

| Рядок | Значення |
|---|---|
| `kernel ШЛЯХ АРГУМЕНТИ` | ядро multiboot2 і його командний рядок |
| `module ШЛЯХ ІМ'Я` | файл, що вантажиться поруч з ядром (`module?` -- пропустити, якщо його немає) |
| `text` | лишити текстовий екран, без графічного режиму |
| `chain signature=XXXXXXXX partition=N` | запустити іншу систему з її власного завантажувального сектора |
| `chain drive=hd1 partition=N` | те саме за номером диска BIOS (номери можуть мінятися) |
| `reboot` | перезавантажити комп'ютер |

`partition=0` -- увесь диск (його MBR, тобто власний завантажувач того диска: GRUB, Windows Boot
Manager, завантажувач BSD), `1..4` -- первинний розділ (завантажувальний сектор розділу). Диск
шукається за підписом MBR (4 байти за зміщенням 440), тому пункт працює, навіть якщо BIOS
переставив диски місцями.

### Інша ОС вручну

```
entry Windows 10
chain signature=0c0ffee0 partition=2

entry OpenBSD
chain signature=5e6f7a8b partition=4

entry Linux (GRUB на другому диску)
chain drive=hd1 partition=0
```

Підпис диска й номер розділу покаже `sudo hxbootctl probe`.

### Автоматичний пошук (`probe yes`)

hxboot читає MBR кожного жорсткого диска (крім того, з якого завантажився) і додає пункти перед
`Reboot`:

| Що на розділі | Пункт |
|---|---|
| NTFS із завантажувальним сектором | Windows |
| тип `0xA5` / `0xA6` / `0xA9` | FreeBSD / OpenBSD / NetBSD |
| FAT із сектором MS-DOS/Windows/FreeDOS | DOS/Windows (FAT) |
| тип `0x83` з кодом у завантажувальному секторі | Linux (GRUB у розділі) |
| нічого з переліченого, але в MBR є код | «Hard disk N (its own boot loader)» |

Диски з GPT поки що пропонуються лише цілим диском (через захисний MBR). UEFI-системи -- етап B2.

## `hxbootctl`

```
hxbootctl [-f ФАЙЛ] list                  меню (* -- пункт за замовчуванням)
hxbootctl show N                          усі рядки пункту N
hxbootctl default N|НАЗВА                 пункт за замовчуванням
hxbootctl timeout СЕКУНДИ
hxbootctl add НАЗВА ЯДРО [АРГУМЕНТИ..]    новий пункт з ядром
hxbootctl add-chain НАЗВА ДИСК РОЗДІЛ     інша ОС: ДИСК -- sda.. або підпис, РОЗДІЛ 0 -- увесь диск
hxbootctl add-reboot [НАЗВА]
hxbootctl module N ШЛЯХ ІМ'Я [--optional]
hxbootctl cmdline N [АРГУМЕНТИ..]         новий командний рядок ядра пункту N
hxbootctl rename N НАЗВА
hxbootctl remove N
hxbootctl move N КУДИ
hxbootctl probe [--add]                   знайти інші ОС на дисках (--add -- додати в меню)
hxbootctl auto on|off                     probe yes|no
hxbootctl check                           чи існують ядра, модулі й диски всіх пунктів
hxbootctl edit                            відкрити файл у $EDITOR (hed) і перевірити
```

* `N` -- номер у меню (з 1, як цифри в самому hxboot) або частина назви.
* `-f ФАЙЛ` -- інший файл, наприклад `/media/STICK/boot/hxboot.cfg`; шляхи `check` рахує від каталогу
  над `boot/`.
* Перед кожною зміною стара версія зберігається як `ФАЙЛ.bak`; коментарі й порядок рядків
  лишаються, як були.
* Читання дисків (`probe`, `add-chain sda ...`) і запис у `/boot` -- від root (`sudo`).

Приклад:

```
$ sudo hxbootctl probe --add
  Windows (sda2)                       chain signature=1a2b3c4d partition=2
      added as entry 5
$ sudo hxbootctl default Windows
$ sudo hxbootctl timeout 5
```

## Ventoy

hxboot знаходить таблицю `ventoy_os_param` (GUID `77772020-2e77-6576-6e74-6f792e6e6574`, пам'ять
`0x80000-0xA0000`) і передає її ядру тегом multiboot2 `0x48585601` разом зі списком фрагментів ISO.
Ядро відкриває ISO на флешці прямо за фрагментами (`loop`-пристрій), тому файлову систему флешки
(exFAT, NTFS) розуміти не треба. Тест `native-vtoy-ntfs`.

## Для розробників

* Розміщення в пам'яті: hxboot -- `0x7C00..0x2F000`, буфер ланцюгового сектора `0x2F000`, буфер
  читання `0x30000` (64 КіБ), multiboot2-info `0x40000`, конфіг `0x50000`, стек `0x7FFF0`.
* Виклики BIOS -- з 32-бітного режиму через перехід у реальний режим (`bios_int` в `entry.S`).
* Гібридна флешка: MBR (`mbr.S`) читає перші 2048 байт hxboot з LBA, яку xtask записує за зміщенням
  `0x1A8`, і стрибає на `0x7C40`.
* ISO пише xtask (`tools/xtask/src/isoimage.rs`): ISO 9660 + Joliet + El Torito + гібридний MBR;
  `grub-mkrescue` і `xorriso` не потрібні.
