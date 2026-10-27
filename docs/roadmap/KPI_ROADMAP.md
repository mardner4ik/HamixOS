# План розширення HamixOS KPI

`docs/MODULES.md` описує те, що вже є: завантажувач ET_REL-модулів, шим
`kernel/src/module/kpi.rs`, крейт `sdk/hamix_kpi` і три драйвери
(`example-nic`, `virtio-gpu`, `intel-display`). Цей документ -- про те, куди
цей KPI росте далі: **шість напрямків**, у порядку, в якому їх має сенс
робити, з файлами, межами й критерієм "зроблено".

**Керівний принцип.** KPI -- це не емуляція `linux/*.h` заради емуляції.
Його мета -- щоб драйвер *не був частиною образу ядра*: машина платить лише
за те залізо, яке в неї є, а драйвер можна написати, замінити або не
завантажити, не перезбираючи ядро. Кожен пункт нижче міряється цим: чи
дозволяє він винести з ядра ще один клас заліза.

Наскрізне обмеження, яке лишається чинним для всіх шести пунктів:
`module::BUDGET_BYTES` -- 20 МіБ на шим плюс усі резидентні модулі, і
кожна нова точка виділення пам'яті має це перевіряти, а не лише перевірка
під час завантаження.

---

## 1. Клас display: від «одна панель» до KMS-подібного ядра -- ЗРОБЛЕНО

> **Статус:** зроблено (ABI дисплея 3, `KPI_VERSION = 6`).
>
> * `ConnectorDesc { kind, id, status, native_w, native_h, edid_len }` і
>   `connectors()` у `DisplayOps`; `set_mode` отримує `connector_id`.
>   `kernel/src/drivers/video/gpu.rs` перекладає ABI 2 у ABI 3 для старих
>   модулів.
> * EDID: `hamix_edid_modes` (`kernel/src/drivers/video/edid.rs`) розбирає
>   детальні дескриптори, стандартні й встановлені таймінги. `intel-display`
>   читає EDID через GMBUS (HDMI/VGA) і DP AUX (eDP/DP, `src/aux.rs`),
>   `virtio-gpu` -- командою `GET_EDID`.
> * Гаряче підключення: `hamix_display_hotplug()` -> ядро перечитує конектори
>   і шле власнику екрана `DisplayChanged`; `hxserver` розсилає
>   `Event::Screen`, `hxwayland` оновлює `wl_output` без перезапуску.
> * `page_flip` + `wait_vblank`: `virtio-gpu` тримає два буфери, `hxserver`
>   малює в задній і перемикає (`pageflip` у конфігурації).
> * `hxsettings` показує всі виходи й режим "Mirror"; `gpuinfo` і hxinit
>   пишуть точну причину, чому модуль дисплея не став прискорювачем.
>
> Перевірено в QEMU: `virtio-gpu` з двома виходами, режими з EDID, зміна
> роздільності й підключення другого виходу під час роботи Nook.


**Де зараз.** `DisplayOps` -- плоска таблиця на *один* вихід: один
framebuffer, один `set_mode(w, h)`, один список частот. Це рівно те, що
потрібно ноутбучній панелі, і рівно те, чого не вистачає всьому іншому.

**Що вже зроблено в цьому напрямку** (ABI дисплея 2, `KPI_VERSION = 2`):

* `mode_list(out, max)` -- модуль сам перелічує підтримувані розміри,
  перший елемент -- рідний режим панелі. `kernel/src/drivers/video/modes.rs`
  більше не вигадує список зі `STANDARD`, якщо модуль його дає.
* `CAP_SCALE` -- модуль повідомляє, що менші режими він робить
  масштабуванням, а не зміною таймінгів панелі.
* `drivers/intel-display` реалізує `set_mode` через panel fitter (gen6-gen8)
  і pipe scaler (gen9): змінюється `PIPESRC` + stride площини, таймінги
  панелі не чіпаються взагалі, PLL не чіпається взагалі.

**Що далі.**

1. `hamix_display_connectors(out, max)` + `ConnectorDesc { kind, id, status,
   native_w, native_h, edid_len }`: eDP/LVDS/HDMI/DP/VGA, підключено чи ні.
   `set_mode` отримує `connector_id` першим аргументом (ABI 3).
2. Таблиця режимів з EDID, а не зі сталого масиву: `drivers/intel-display/src/edid.rs`
   вже читає блок 128 байт через GMBUS -- лишається розібрати детальні
   дескриптори й стандартні таймінги і віддати їх у `mode_list`.
3. Гаряче підключення: `hamix_display_hotplug()` -- зворотний виклик модуля
   в бік ядра, щоб `modes.rs` перечитав конектори, а `hxwayland` отримав
   подію про зміну геометрії без опитування `SYS_FB_GENERATION`.
4. `page_flip(surface)` + `wait_vblank()`: подвійна буферизація замість
   `flush` по брудному прямокутнику. Це те, що прибирає tearing у Nook і
   робить 60 FPS дешевшим за поточний шлях "CPU-композит + transfer".

**Зроблено, коли:** у ноутбука з зовнішнім монітором `hxsettings` показує
два виходи, режими беруться з EDID кожного, а `hxwayland` перемальовується
після підключення кабелю без перезапуску.

---

## 2. Переривання: `hamix_request_irq`, MSI та MSI-X -- ЗРОБЛЕНО

> **Статус:** зроблено (KPI 3). `kernel/src/drivers/irq.rs` -- таблиця
> обробників на вектор, лічильники, черга нижніх половин і watchdog;
> `kernel/src/arch/x86_64/idt.rs` має точки входу на вектори 34-47 і
> 0x50-0x6F, які ведуть у `irq::dispatch`. У KPI додані
> `hamix_request_irq`, `hamix_free_irq`, `hamix_pci_msi_enable`,
> `hamix_schedule_work`, `hamix_in_interrupt`.
>
> * **MSI** (capability 0x05) програмується першим: адреса
>   `0xFEE0_0000 | (apic_id << 12)`, дані -- вектор, INTx вимикається.
> * Якщо MSI немає (або є лише MSI-X, capability 0x11, таблиця якої ще не
>   програмується), береться **лінія PIC**: `irq::request_legacy` знімає
>   маску з 8259 і ставить обробник на вектор `32 + line`.
> * Обробник виконується з вимкненими перериваннями, повертає
>   `IRQ_HANDLED`/`IRQ_NONE`; нерозпізнані переривання рахуються окремо.
>   Довга робота віддається в чергу `hamix_schedule_work`, яку розбирає
>   потік ядра **з** BKL.
> * `/proc/interrupts` і команда `interrupts` показують вектор, вид
>   (pic/msi), лічильник і модуль-власник.
>
> Перевірено в QEMU: `drivers/virtio-gpu` бере лінію 11 (вектор 0x2b),
> `/proc/interrupts` показує зростаючий лічильник, spurious -- 0.
>
> Не зроблено в межах пункту: програмування **таблиці MSI-X** (пристрій із
> самим лише 0x11 лишається на INTx) і IOAPIC (використовується 8259).

**Де зараз (початковий опис).** Переривань у KPI немає взагалі -- це записано в
`docs/MODULES.md` як свідома відсутність. IDT у `kernel/src/arch/x86_64/idt.rs`
веде вектори 34-47 у заглушки, а кожен драйвер (e1000, Yukon, AR9285,
virtio-gpu) *опитує* залізо з `poll`-колбека, який реєструє
`hamix_claim_device`.

Це головна причина, чому "портований Linux-драйвер" досі неможливий: у
Linux-драйвері обробник переривання -- не деталь, а каркас.

**Що робити.**

1. Реальна диспетчеризація IRQ у ядрі: таблиця `Vec<Handler>` на вектор,
   підтримка спільних рівневих ліній (PCI INTx), EOI до APIC, лічильники у
   `/proc/interrupts`.
2. `hamix_request_irq(handle, handler, context) -> i32` /
   `hamix_free_irq(handle)`. Обробник виконується з вимкненими перериваннями
   і **без BKL** -- отже йому доступна тільки підмножина KPI (MMIO, атоміки,
   `hamix_printk`), а не `hamix_kmalloc`.
3. Нижня половина: `hamix_schedule_work(fn, context)` -- черга, яку розбирає
   потік ядра з BKL. Саме туди Linux-драйвер віддасть те, що в нього в
   `napi_poll`/`tasklet`.
4. MSI/MSI-X: читання capability 0x05/0x11 у `kernel/src/drivers/pci.rs`,
   програмування адреси/даних, `hamix_pci_msi_enable(handle, vectors)`.
   Для virtio-gpu і e1000 це безпосередній виграш -- зникає опитування.

**Межа.** Обробник у модулі, який зависне, вішає машину. Тому разом з цим
пунктом потрібен watchdog із пункту 5, а не окремо після нього.

**Зроблено, коли:** `example-nic` приймає пакети без `poll`-колбека, а
`/proc/interrupts` показує зростаючий лічильник на його векторі.

---

## 3. Шини й класи пристроїв: не тільки PCI, не тільки мережа й екран -- ЗРОБЛЕНО

> **Статус:** зроблено.
>
> * `DeviceHandle` (PCI / платформа), `hamix_pci_find_class`,
>   `hamix_platform_find`, `hamix_bus_publish` і рядки `.ids` виду
>   `class:CC:SS:PP` та `<bus> vendor:device`.
> * `CLASS_BLOCK` (`hamix_register_block`) -- `drivers/ahci` замінює
>   вбудований AHCI; модулі з `early = 1` вантажаться з `/boot/kmods.tar`
>   (GRUB `module2 ... kmods`) ще до монтування кореня, тож встановлена
>   система стартує з `ahci.ko`.
> * `CLASS_INPUT` (`hamix_input_key/pointer/absolute`), `CLASS_AUDIO`
>   (`hamix_register_audio`).
> * `CLASS_USB_HCD`: `hamix_usb_register_hcd`, `hamix_usb_add_device`,
>   `hamix_usb_remove_device`, `hamix_usb_hid_report`. `drivers/xhci` --
>   xHCI як модуль: передача від BIOS, скидання, DCBAA і scratchpad-и,
>   кільця команд/подій, скидання порту, Address Device, дескриптори,
>   HID boot-протокол для клавіатур і мишей, MSI/INTx, гаряче підключення.
>   Працює на x86_64, aarch64 і riscv64 (QEMU `qemu-xhci`).
> * `CLASS_NETWORK` з даними: `hamix_register_netdev` (`NetOps` з
>   `transmit`), `hamix_net_receive`, `hamix_net_carrier`; пристрій
>   з'являється в `ifconfig` як звичайний `ethN`.


**Де зараз.** `hamix_claim_device(class, ...)` знає два класи
(`CLASS_NETWORK`, `CLASS_DISPLAY`) і одну шину: `PciHandle` -- це буквально
bus/device/function. Усе інше залізо (AHCI, HDA, UHCI/USB-HID, тачпад,
клавіатура) статично влінковане в ядро.

**Що робити.**

1. Шинно-нейтральний дескриптор: `DeviceHandle { bus_kind, id, .. }`, де
   `bus_kind` -- PCI, USB, платформа (порти I/O + IRQ), віртуальна.
   `PciHandle` лишається як окремий випадок, щоб не ламати наявні модулі.
2. Нові класи з такою ж таблицею можливостей, як у дисплея:
   * `CLASS_BLOCK` -- `BlockOps { read, write, flush, capacity, ... }`,
     реєстрація в `kernel/src/drivers/block.rs`. Перший кандидат на винос --
     `ahci.rs` (524 рядки в ядрі), другий -- NVMe, якого зараз просто немає.
   * `CLASS_INPUT` -- `InputOps` з подіями у форматі, який уже зрозумілий
     `kernel/src/drivers/input/`. Виносить USB-HID і тачпад.
   * `CLASS_AUDIO` -- `AudioOps { open, write, volume }`; виносить `hda.rs`
     (724 рядки) і відкриває шлях до USB-аудіо.
   * `CLASS_USB_HCD` -- контролер як модуль, щоб UHCI/EHCI/xHCI не жили в
     ядрі; xHCI зараз видалений саме тому, що йому не було де жити.
3. `hamix_register_bus` для контролера, який сам публікує дочірні пристрої
   (USB-хаб, virtio-шина), щоб модуль міг спричинити завантаження іншого
   модуля.

**Зроблено, коли:** `ahci` завантажується з `/lib/modules/ahci.ko`, система
з нього ж і стартує, а `modules` показує його в бюджеті.

---

## 4. `linuxkpi`: шар імен, на якому компілюється Linux-драйвер -- ЗРОБЛЕНО

> **Статус:** зроблено.
>
> * `sdk/hamix_linuxkpi/include` -- заголовки `linux/*.h`, `net/*.h`,
>   `asm/*.h` над одним `hamix/linuxkpi.h`; `sdk/hamix_linuxkpi/src/linuxkpi.c`
>   -- рантайм: `printk`/`vsnprintf` (з `%pM`), `kmalloc`, спінлоки й
>   м'ютекси, `timer_list`, `work_struct`/`delayed_work`, модель `pci_dev`
>   і `pci_driver` з `probe/remove`, конфігураційний простір і PCIe
>   capability, `request_irq` поверх `hamix_request_irq` (MSI або INTx),
>   `dma_alloc_coherent`/`dma_map_single`, `sk_buff` з DMA-пулом, NAPI
>   через чергу робіт, `net_device` -> `hamix_register_netdev`.
> * `build.sh` збирає C-модулі clang-ом (`drivers/<name>/src/*.c` без
>   `Cargo.toml`), лінкує `ld.lld -r` в один ET_REL і генерує `.ids` з
>   `MODULE_DEVICE_TABLE` (`tools/linuxkpi/device-ids.py`).
> * `drivers/e1000e` -- драйвер Intel e1000e з Linux v6.6. Від апстріму
>   відрізняється лише видаленим: без `ethtool.c` і `ptp.c` і без трьох
>   рядків, що їх викликали. `tools/linuxkpi/check-upstream.sh drivers/e1000e`
>   завантажує апстрім і перевіряє, що немає жодного доданого чи
>   переписаного рядка.
>
> Перевірено в QEMU q35 (82574L): модуль бере пристрій замість вбудованого
> `e1000`, MSI-переривання, лінк 1000 Мбіт/с, DHCP, завантаження 8 МБ по HTTP
> зі збігом SHA-256; Telegram Desktop через нього входить на сервери.


**Де зараз.** KPI -- HamixOS-подібний за формою: `hamix_readl`, `hamix_kmalloc`,
`hamix_pci_bar`. Це зручно писати з нуля і неможливо використати для
`r8169.c` без переписування кожного рядка.

**Що робити.** Окремий крейт `sdk/hamix_linuxkpi`, який **не додає нічого в
ядро** -- це чисто заголовковий шар над наявними символами:

1. Пам'ять і рядки: `kmalloc/kfree/kzalloc/krealloc`, `GFP_*` (ігноруються),
   `memcpy_fromio`, `ioread32/iowrite32`.
2. Синхронізація: `spinlock_t`, `mutex`, `atomic_t`, `completion` -- поверх
   `spin::Mutex` і атоміків; на однопроцесорній системі без витіснення в
   ядрі більшість із них вироджується в `cli/sti`.
3. Модель пристрою: `struct device`, `struct pci_dev`, `pci_driver`
   з `probe/remove` і `MODULE_DEVICE_TABLE`, що транслюється в `module.ids`.
4. DMA API: `dma_alloc_coherent`, `dma_map_single`, `dma_unmap_single`
   поверх `hamix_dma_alloc` (зараз віртуальна адреса == фізична, тож
   відображення тривіальне -- але API має бути правильним *до* того, як
   з'явиться IOMMU).
5. Час і черги: `msleep`, `udelay`, `jiffies`, `schedule_work`,
   `workqueue` -- поверх пункту 2.

**Межа, яку треба тримати чесно.** Мета -- "драйвер, з якого викинули
sysfs/debugfs/firmware-loader, компілюється й працює", а **не** "vanilla
`.c` з дерева Linux збирається без змін". Друге -- це FreeBSD-масштаб
роботи; заявляти його як ціль було б нечесно.

**Зроблено, коли:** один реальний PCI-NIC (`r8169` або `e1000e`) їздить з
`/lib/modules`, і його C-код відрізняється від апстріму тільки видаленими
шматками, а не переписаними.

---

## 5. Ізоляція, бюджети й захист від поганого модуля -- ЗРОБЛЕНО

> **Статус:** зроблено, усі п'ять підпунктів.
>
> 1. **W^X.** `paging::init` вмикає EFER.NXE (CPUID 0x80000001 EDX біт 20),
>    `paging::protect_kernel_range` розбиває 2-МіБ сторінку ядра на 4 КіБ і
>    ставить права посторінково, `Image::protect` робить `.text` RX, а решту
>    RW+NX. Стан видно в `modinfo` (`pages`) і `/sys/module/<name>/protection`.
> 2. **Межі відображення.** `hamix_ioremap` більше не повертає фізичну
>    адресу як є: діапазон має лежати всередині BAR-а якогось PCI-пристрою
>    (список знімається один раз перед завантаженням модулів,
>    `kpi::scan_windows`) або всередині framebuffer'а. Додатково, якщо BAR
>    лежить вище за ідентично відображені 4 ГіБ, вікно **домапується**
>    (`paging::map_kernel_mmio`, 2-МіБ сторінки, uncached) -- саме через це
>    раніше падало ядро на машині з 4 ГБ RAM, де QEMU кладе 64-бітний BAR
>    virtio-gpu на 0x1_4000_0000.
> 3. **Бюджет на модуль.** `/lib/modules/<name>.conf` -> `limit=`; кожен
>    `kmalloc`/`kzalloc`/`dma_alloc` рахується **на модуль**
>    (`kpi::allocated_by`) і відмовляє, коли ліміт вичерпано.
> 4. **Watchdog.** `poll`-колбек і обробник переривання міряються;
>    понад `irq::STALL_MS` (250 мс) -> `module::mark_stalled`, і найближчий
>    `poll_devices` знімає лінію переривання, скидає claim-и й вивантажує
>    модуль замість того, щоб система зависла.
> 5. **Підпис.** `/lib/modules/<name>.sig` -- SHA-256 файла (`build.sh`
>    пише його через `sha256sum`), `module::check_signature` звіряє його
>    власною реалізацією SHA-256 (`kernel/src/module/digest.rs`). Поки що
>    **режим попередження**: розбіжність пишеться в `dmesg`, стан
>    (`ok`/`bad`/`unsigned`) видно в `modinfo`.

**Де зараз (початковий опис).** Модуль виконується в ring 0 з тими ж правами, що ядро; сторінки
образу RWX (`docs/MODULES.md`: NX не увімкнено, `boot.S` ставить лише
EFER.LME); `hamix_ioremap` повертає фізичну адресу як є, тож модуль може
відобразити **будь-що**; `poll`-колбек без таймауту; вивантаження живого
драйвера не зупиняє DMA.

Для системи, яка завантажує код із диска за збігом PCI id, це найслабше
місце в усьому KPI.

**Що робити.**

1. W^X для образу модуля: `.text` -- RX, `.rodata` -- R, `.data`/`.bss` --
   RW. Потребує ввімкнути NX (EFER.NXE у `boot.S`) і посторінкові права в
   `kernel/src/module/image.rs`.
2. Перевірка діапазонів: `hamix_ioremap` дозволяє тільки BAR-и пристроїв,
   які модуль **claim**-нув, і тільки в межах їхньої довжини;
   `hamix_dma_alloc` -- уже в бюджеті, але має ще й запам'ятовувати
   діапазони, щоб `hamix_dma_free` не звільняв чужу пам'ять.
3. Бюджет на модуль, а не тільки глобальний: `module.limits` поряд із
   `module.ids`, щоб один драйвер не з'їв усі 20 МіБ.
4. Watchdog: якщо `poll`/IRQ-обробник модуля не повернувся за N мс, модуль
   позначається несправним, його claim-и скидаються, і система далі працює
   без нього (замість зависання).
5. Підпис: `module.sig` поряд із `.ko`, перевірка тим самим кодом, який
   `apps/pantry/src/crypto.rs` уже використовує для пакетів. Спершу -- режим
   попередження в `dmesg`, і тільки потім -- обов'язковий.

**Зроблено, коли:** модуль, який навмисно пише за межі свого BAR або
зациклюється в `poll`, дає повідомлення в `dmesg` і вивантажується, а не
вішає машину.

---

## 6. Життєвий цикл, гаряче вивантаження й діагностика -- ЗРОБЛЕНО

> **Статус:** зроблено, крім повного `probe` на кожен пристрій (модуль і далі
> сам перебирає свої id у `init`).
>
> 1. `module!` приймає `version = "..."`, `suspend = ...`, `resume = ...`;
>    ядро шукає `hamix_module_suspend`/`hamix_module_resume` і зберігає
>    версію з `hamix_module_version`.
> 2. **Порядок вимикання.** `unload` виконує `exit`, далі знімає лінію
>    переривання (`irq::release`), скидає claim-и, забуває облік пам'яті і
>    лише потім звільняє образ; `Image::drop` ще й повертає права сторінок.
> 3. `suspend_all`/`resume_all`; `sys_power` (reboot/poweroff) тепер
>    спершу присипляє модулі, тож драйвер зупиняє DMA до скидання машини.
> 4. **Діагностика:** `/sys/module/<name>/{version,coresize,limit,signature,
>    protection,state,devices,parameters/*}`, `/proc/modules_detail`,
>    команди `modinfo` та `interrupts`, рівні логів
>    `hamix_dev_log` (`dev_err`/`dev_warn`/`dev_info`/`dev_dbg`) з фільтром
>    `loglevel=` у `<name>.conf`, лічильники переривань на модуль.
> 5. **Параметри:** `hamix_param_u32(name, fallback)` читає
>    `/lib/modules/<name>.conf`. Уже використовується: `virtio-gpu` --
>    `irq=0` вимикає переривання, `intel-display` -- `scaler=0` вимикає
>    масштабування, `edid=0` -- читання EDID.

**Де зараз (початковий опис).** Модуль завантажується один раз на старті (`modules`-юніт у
`kernel/src/hxinit.rs`), `module::unload` формально існує, але `docs/MODULES.md`
чесно каже: ніщо не зупиняє пристрій, який далі пише в звільнену DMA-пам'ять.
Діагностика -- `modules` і `/proc/modules`.

**Що робити.**

1. Повний життєвий цикл замість `init`/`exit`: `probe(device)` на кожен
   збіг id (модуль може вести кілька пристроїв), `remove(device)`,
   `suspend`/`resume`. `module!` розширюється, старі `init/exit` лишаються
   як окремий випадок для модулів без пристроїв.
2. Порядок вимикання: `remove` спершу зупиняє DMA/движок, потім ядро
   звільняє пам'ять. Без цього гаряче вивантаження лишається небезпечним, як
   і зараз.
3. `suspend`/`resume` -- передумова для сну машини взагалі; для
   `intel-display` це означає зберегти й відновити `PIPESRC`, scaler і
   таймінги (код відновлення вже є в `mode::restore` і `exit`).
4. Діагностика: `/sys/module/<name>/{version,refcnt,parameters,claims}`,
   `modinfo`, рівні логів (`hamix_dev_err/warn/info/dbg`) з фільтром у
   `dmesg`, лічильники IRQ і байтів DMA на модуль.
5. Параметри модуля: `module.conf` поряд із `.ko` і
   `hamix_param_u32(name, default)` -- щоб `intel-display` можна було
   змусити ігнорувати EDID або вимкнути scaler без перезбірки.

**Зроблено, коли:** `rmmod intel-display && insmod intel-display` на живій
системі повертає картинку, а не вішає її.

---

## Порядок

Пункти 1 і 6 можна робити паралельно -- вони не конфліктують. Пункт 2
(переривання) -- передумова для 3 і 4 і має йти перед ними. Пункт 5
неподільний із 2: не можна давати модулю обробник переривання й не давати
ядру способу його зупинити. Пункт 4 -- останній за змістом: він має сенс
лише тоді, коли під ним уже є переривання, DMA-межі та класи пристроїв.

```
1 (display) ──┐
              ├── незалежні
6 (lifecycle)─┘

2 (IRQ) ── 5 (ізоляція) ── 3 (шини/класи) ── 4 (linuxkpi)
```
