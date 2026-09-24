# Портування ядра HamixOS на aarch64 і riscv64

Мета -- не "ще одна гілка ядра", а одне ядро, де все, що не залежить від
процесора (VFS, hext, мережа, syscall-и, планувальник, драйвери virtio),
компілюється під три архітектури, а різниця живе тільки в `kernel/src/arch/`.
Цільові машини для розробки: QEMU `virt` для обох архітектур (aarch64 --
`-M virt -cpu cortex-a72`, riscv64 -- `-M virt` з OpenSBI). Реальне залізо
(Raspberry Pi 4/5, VisionFive 2) -- після пункту 6.

Цілі збірки:

| Архітектура | Rust target                     | Адреса завантаження | Прошивка        |
|-------------|---------------------------------|---------------------|-----------------|
| x86_64      | `kernel/x86_64-hamix_os.json`   | 1 MiB (multiboot2)  | GRUB            |
| aarch64     | `aarch64-unknown-none-softfloat`| `0x4020_0000`       | немає (`-kernel`)|
| riscv64     | `riscv64imac-unknown-none-elf`  | `0x8020_0000`       | OpenSBI         |

Ядро на нових архітектурах збирається без FP/SIMD (softfloat / без `d`),
щоб у обробниках переривань і перемиканні контексту не зберігати FP-регістри
ядра -- так само, як Linux.

---

## 1. Шар `arch` і завантаження до Rust на обох архітектурах -- ЗРОБЛЕНО

* `kernel/src/arch/mod.rs` експортує однаковий набір примітивів для всіх
  архітектур: `hlt`, `enable_interrupts`, `disable_interrupts`,
  `interrupts_enabled`, `without_interrupts`, `HEAP_LIMIT`.
  Спільний код (`memory::frame`, `memory::heap`) звертається тільки до
  `crate::arch::*`, а не до `crate::arch::x86_64::*`.
* aarch64 (`arch/aarch64/`): спуск EL3/EL2 -> EL1, доступ EL1 до таймера,
  паркування вторинних ядер за `MPIDR_EL1`, збереження вказівника на DTB,
  очищення BSS, стек, `kernel_main(dtb)`.
* riscv64 (`arch/riscv64/`): вхід від OpenSBI (`a0` = hart id, `a1` = DTB),
  лотерея завантажувального hart'а (`amoadd`), решта hart'ів паркуються у
  `wfi`, `gp`, стек, BSS, `kernel_main(hart, dtb)`.
* Спільний ранній шлях `kernel/src/early/`: консоль (PL011 / NS16550A),
  `kprintln!`, обробник паніки, `alloc_error_handler`.
* `./build.sh aarch64`, `./build.sh riscv64` -- збірка і команда запуску QEMU.

## 2. Device tree, карта фізичної пам'яті, фреймовий алокатор і heap -- ЗРОБЛЕНО

* `kernel/src/fdt.rs` -- парсер FDT без алокацій: заголовок, memreserve,
  обхід вузлів з `#address-cells`/`#size-cells`, пошук за шляхом (з
  необов'язковою unit-адресою), `reg`, `compatible`, `/aliases`.
* `memory::init_devicetree`: RAM з вузлів `device_type = "memory"`,
  резервування ядра (і всього, що лежить нижче за нього в банку: DTB на
  aarch64, OpenSBI на riscv64), самого DTB, memreserve, `/reserved-memory`,
  initrd з `/chosen` (реєструється як модуль `initrd`), `bootargs` ->
  `memory::cmdline()`. Консоль перемикається на `stdout-path`.
* Multiboot2-розбір виїхав у `memory/multiboot.rs` (тільки x86_64), а
  `frame.rs`/`heap.rs` тепер спільні для трьох архітектур.
* aarch64: рання тотожна трансляція (`arch/aarch64/mmu.rs`, 1 GiB блоки,
  MAIR: Device-nGnRnE / Normal WB) і ввімкнені кеші -- без MMU вся пам'ять
  вважається Device, а на ній не працюють ексклюзивні доступи (спін-локи).
* Самоперевірка під час старту: `Vec`/`BTreeMap`/`String`, великий
  (>256 KiB) шлях алокатора, `heap_trim`, лічильники вільних фреймів.
* Ядро aarch64 лінкується на `0x4020_0000`, а не `0x4008_0000`: для
  ELF-образу QEMU кладе DTB на початок RAM тільки якщо він поміщається
  перед ядром, а DTB `virt` займає рівно 1 MiB.
* Перевірено в QEMU 11.1 (пункти 1-4): aarch64 (cortex-a72/a53/max, EL1 і
  `virtualization=on`, GICv2/GICv3, 512M-4G, `-smp 4`) і riscv64 (OpenSBI, 512M-2G,
  `-smp 4`, `-initrd`). Для aarch64 `-initrd` запрацював у пункті 6: QEMU
  передає initrd тільки Linux-образам, тому ядро тепер має `Image`-заголовок.

```
./build.sh aarch64
qemu-system-aarch64 -M virt -cpu cortex-a72 -m 512M -nographic \
    -kernel target/aarch64-unknown-none-softfloat/release/kernel
./build.sh riscv64
qemu-system-riscv64 -M virt -m 512M -nographic \
    -kernel target/riscv64imac-unknown-none-elf/release/kernel
```

## 3. Винятки, переривання і таймер -- ЗРОБЛЕНО

* aarch64 (`arch/aarch64/traps.rs`): таблиця векторів `VBAR_EL1` (16 входів
  по 0x80), кадр з x0-x30, `ELR`/`SPSR`/`ESR`/`FAR`/`SP_EL0`, розбір класу
  `ESR_EL1` (brk, svc, data/instruction abort), друк кадру при паніці.
* aarch64 (`gic.rs`): GICv2 (`arm,cortex-a15-gic`, `arm,gic-400`) і GICv3
  (`arm,gic-v3`: пробудження редистриб'ютора, системні регістри `ICC_*`,
  `ICC_SRE_EL2` виставляється при старті з EL2). Адреси й номер переривання
  пристрою -- з DTB (`interrupts = <SPI/PPI n flags>`).
* aarch64 (`timer.rs`): віртуальний generic timer (`CNTV_*`), номер PPI з
  вузла `arm,armv8-timer`, частота з `CNTFRQ_EL0`.
* riscv64 (`arch/riscv64/traps.rs`): `stvec` (direct), кадр з 31 регістром +
  `sepc`/`sstatus`/`scause`/`stval`; `ebreak` пропускається з урахуванням
  стиснених інструкцій.
* riscv64 (`plic.rs`): контекст S-режиму для свого hart'а шукається по
  `interrupts-extended` (phandle контролера `riscv,cpu-intc` + лінія 9), а
  не "2*hart+1" -- з 4 hart'ами QEMU стартував hart 1 і отримав контекст 3.
* riscv64 (`timer.rs`, `sbi.rs`): SBI `TIME` (з відкатом на legacy
  `set_timer`), `rdtime`, `timebase-frequency` з DTB.
* Спільне: `arch::irqtab` (таблиця обробників і лічильники, 1024 лінії),
  `arch::irq::{enable, interrupt_of}`, `arch::timer::{init, ticks, now_ns,
  hz}`, `arch::traps::{breakpoint, probe_read}`. `probe_read` -- читання,
  яке переживає відмову сторінки (обробник пропускає інструкцію і повертає
  адресу відмови) -- на цьому пізніше тримається `copy_from_user`.
* Консоль отримує переривання прийому (PL011 `IMSC`, NS16550 `IER`), тобто
  перевірено реальний шлях IRQ від пристрою, а не лише таймер.
* x86_64 лишається на `drivers/irq.rs`; переведення його на той самий
  `arch::irqtab` -- разом із пунктом 5, коли `task/` почне викликати спільний
  API.

## 4. Сторінкова пам'ять і адресні простори -- ЗРОБЛЕНО (без вищої половини)

* **Змінене рішення:** ядро на всіх трьох архітектурах лишається тотожно
  відображеним (фізична адреса == віртуальна), а адресний простір ділиться
  так само, як на x86_64: верхній запис 0 -- тотожна мапа RAM і MMIO, 1 --
  користувач, 2 -- vmalloc. Причина: драйвери, `task/`, `net/`, модулі
  KPI передають фізичні адреси як вказівники (і навпаки) у сотнях місць;
  перехід у вищу половину -- це окремий рефакторинг усього ядра (x86_64
  теж), а не частина портування. Пункти 5-6 від цього тільки виграють: той
  самий код працюватиме без змін.
* aarch64 (`arch/aarch64/paging.rs`): 4 рівні по 4 KiB, 48-бітний VA
  (`T0SZ = 16`). L0[0] -- 1 GiB блоки: `Normal WB` там, де DTB каже, що є
  RAM, `Device-nGnRnE` + `PXN/UXN` на решті. L0[1] -- процес (`nG`,
  `AP[1]`, `UXN` без `EXEC`, `PXN` завжди). L0[2] -- vmalloc. TLB --
  `tlbi vaae1is`/`vmalle1is` (широкомовні, для SMP вистачає).
* riscv64 (`arch/riscv64/paging.rs`): Sv39 (його підтримують і QEMU, і
  VisionFive 2, на відміну від Sv48): гігасторінки 0-128 GiB тотожно,
  128-192 GiB -- користувач, 192-224 GiB -- vmalloc (32 L1-таблиці виділені
  наперед і спільні). `SUM` увімкнено, `A/D` ставляться одразу (без Svadu
  немає зайвих відмов). Біт `OWNED` -- у RSW (біт 8).
* `AddressSpace::{new, map, alloc_range, unmap, translate, activate,
  destroy}` з однаковим API на aarch64/riscv64; `destroy` спершу
  перемикається на корінь ядра, якщо простір активний.
* `memory::vmalloc` (див. `docs/OPTIMIZATION.md`) працює на всіх трьох
  архітектурах через `arch::kmap`; самоперевірка при старті розтягує `Vec`
  з 300 KiB до 4 MiB без копіювання.
* Самоперевірка: відмова на незмапленій сторінці перехоплюється, запис через
  користувацьку мапу видно через тотожну, `unmap` знову дає відмову, другий
  простір не бачить сторінок першого, після `drop` -- 0 загублених кадрів.
* Не зроблено: ASID (зараз кожне `activate` скидає весь TLB), SMP-
  розсилка TLB на riscv64 (SBI `RFENCE`) -- обидва потрібні лише з пунктом 5.

## 5. Процеси, перемикання контексту і syscall-и -- ЗРОБЛЕНО

Ядро тепер одне: `task/`, `syscall/` (нативний і Linux ABI), `fs/`, `net/`,
`hxinit`, `vt`, `login`, модулі KPI і драйвери збираються під усі три
архітектури. Від x86_64 у спільному коді лишилися тільки перевірки
`crate::arch::io::PORTS` там, де драйвер справді ходить у порти вводу-виводу.

* **Спільний API `crate::arch`**: `context` (кадр переривання, перемикання,
  трамплін у user-mode), `paging`, `pte`, `smp`, `platform` (reset/poweroff,
  назва CPU, `/proc/cpuinfo`, лічильник тактів, апаратний RNG), `io`
  (порти на x86_64, заглушки деінде), `delay_us`/`delay_ms`, `idle_wait`,
  `start_tick`, `MACHINE`.
* **Один `AddressSpace` на три архітектури** (`memory/aspace.rs`): лінива
  пам'ять, `fork`-копія, спільні сторінки, framebuffer -- однаковий код, а
  різниця тільки в кодувальнику PTE (`arch::<a>::paging::pte`): рівні,
  індекси, біти `VALID`/`OWNED`/`LAZY`, write-combining (Normal-NC на
  aarch64), синхронізація I-кешу (`dc cvau`+`ic ialluis`, `fence.i`).
  На riscv64 користувацькі адреси -- 128-192 ГіБ (Sv39), тому
  `apps/link.ld` бере базу з `--defsym=HAMIX_USER_BASE`.
* **Перемикання контексту**: aarch64 зберігає x19-x30 + `DAIF`, riscv64 --
  `ra`, s0-s11 + `sstatus.SIE`. TLS: `TPIDR_EL0` зберігається/відновлюється
  при перемиканні, на riscv64 `tp` живе в кадрі (`CLONE_SETTLS` пише його
  прямо в кадр дитини).
* **Вхід у ядро**: aarch64 -- окремі вектори для EL1 і EL0, на EL0 ще й
  q0-q31/`FPSR`/`FPCR` (ядро softfloat, тому FP користувача лише в
  кадрі); riscv64 -- `sscratch` тримає вершину стеку ядра поки процес у
  U-mode, f0-f31/`fcsr` зберігаються при кожній пастці з U-mode,
  `sstatus.FS` увімкнено назавжди.
* **Syscall**: `svc #0` (номер у `x8`), `ecall` (номер у `a7`). Нативні
  `SYS_HAMIX_*` мають ті самі номери на всіх архітектурах, а Linux-процеси
  проходять таблицю `syscall/linux/generic.rs` (asm-generic -> x86_64) з
  трьома поправками: порядок аргументів `clone`, прапорці `O_*` на aarch64
  (`O_DIRECTORY`, `O_NOFOLLOW`, `O_DIRECT`, `O_LARGEFILE` мають інші біти) і
  `riscv_flush_icache`. `struct stat` (128 байт замість 144) та
  `epoll_event` (16 байт, без `packed`) кодуються під архітектуру.
* **Сигнали**: `syscall/linux/sigframe/{x86_64,aarch64,riscv64}.rs`,
  кадри сумісні з Linux (`rt_sigframe`, `fpsimd_context` на aarch64,
  `sc_regs`+D-стан на riscv64). Якщо програма не дала `SA_RESTORER`
  (glibc, riscv64-musl), ядро кладе трамплін `rt_sigreturn` на сторінку
  одразу над стеком. Перезапуск syscall-у повертає `orig_x0`/`orig_a0`.
* **ELF**: `EM_AARCH64`/`EM_RISCV`, `AT_HWCAP` (з `ID_AA64ISAR0_EL1` на
  aarch64, `imafdc` на riscv64), `AT_PLATFORM` = назва архітектури.
* **`hamix_std`**: `svc`/`ecall` і `_start` для обох архітектур,
  `sys::ARCH`; застосунки збираються вбудованими target-ами
  `aarch64-unknown-none` і `riscv64gc-unknown-none-elf` -- усі 18 програм
  з `apps/` збираються без змін.
* **Перший процес**: ранній старт (`early/`) після самоперевірок передає
  керування тому самому `hxinit`, що й на x86_64; корінь -- hext-образ з
  initrd, `login` на tty1 працює через UART (вивід VT0 дзеркалиться в
  послідовний порт, байти з UART стають натисканнями клавіш).

Перевірено в QEMU `virt`: вхід у систему, `hsh`, `fetch`, конвеєри,
`startx` (Nook, hxterm, Videos), а також Linux-програми з Alpine (busybox +
musl, динамічне завантаження): `sh -c`, арифметика, `&` + `wait`,
`trap`/`kill -USR1`, `awk` з плаваючою комою.

Обмеження: статичні не-PIE Linux-бінарники (ET_EXEC на 0x400000) не
запускаються на жодній архітектурі -- нижні 512 ГіБ (128 ГіБ на riscv64)
зайняті тотожною мапою ядра (див. пункт 4). Одне ядро (без SMP) на
aarch64/riscv64.

## 6. Драйвери платформи і Hamix KPI -- ЗРОБЛЕНО

* **virtio** (`kernel/src/drivers/virtio/`): спільна черга (split ring з
  вільним списком дескрипторів), транспорти MMIO (legacy v1 і v2) та
  PCI (modern, capability-и), драйвери `blk` (диск `vdX` у блоковому шарі,
  hext-корінь `root=auto` працює як з SATA), `net` (`NetDevice` для
  smoltcp, DHCP), `input` (клавіатура, миша, планшет; коди Linux ->
  scancode set 1) і `gpu` (MMIO, framebuffer-консоль і Nook, апаратний
  курсор, зміна режиму). Ті самі драйвери працюють і на x86_64 через
  virtio-PCI; GPU на PCI лишається за модулем `drivers/virtio-gpu`.
* **PCIe ECAM** (`pci-host-ecam-generic`): конфігураційний простір через
  MMIO, розстановка BAR-ів із вікна `ranges` (без прошивки їх ніхто інший не
  програмує), маршрутизація INTx через `interrupt-map` у GIC/PLIC.
  `irq::request_legacy` на aarch64/riscv64 веде лінію в `arch::irqtab`.
* **Пристрої з device tree**: RTC (`arm,pl031`, `google,goldfish-rtc`),
  PSCI (`SYSTEM_OFF`/`SYSTEM_RESET`) і SBI SRST для вимкнення й
  перезавантаження.
* **Hamix KPI на нових архітектурах**: `kernel/src/module/reloc.rs` --
  `R_AARCH64_ABS64/ABS32/PREL*/ADR_PREL_PG_HI21/ADD_ABS_LO12_NC/LDST*_LO12/
  CALL26/JUMP26` (з вінірами `ldr x16`/`br x16`, якщо ціль далі 128 МіБ),
  `CONDBR19/TSTBR14`, GOT (`ADR_GOT_PAGE`/`LD64_GOT_LO12_NC`);
  `R_RISCV_64/32/CALL/CALL_PLT/PCREL_HI20/PCREL_LO12_I/S/GOT_HI20/BRANCH/
  JAL/RVC_*/ADD*/SUB*/SET*` (два проходи, щоб `PCREL_LO12` знайшов свій
  `HI20`), `RELAX`/`ALIGN` ігноруються. Після релокацій -- синхронізація
  I-кешу. `build.sh` збирає модулі профілем `module` під target ядра;
  `hamix_kpi` без змін в API. Перевірено: той самий `drivers/virtio-gpu`
  вантажиться на aarch64 і riscv64, бере переривання з GIC/PLIC.
* **`build.sh aarch64|riscv64`** збирає ядро, усі застосунки й модулі,
  root-образ `out/<arch>/hext.img`. Для aarch64 ще й `Image` з заголовком
  arm64 (QEMU `-kernel` передає `-initrd`), PE/COFF-заголовком і
  **EFI-стабом** (`arch/aarch64/efi.rs`): стаб бере DTB з таблиці
  конфігурації UEFI, читає `hext.img` з того самого розділу, виходить з boot
  services, копіює ядро на `0x4020_0000` і передає initrd у регістрах.
  `out/aarch64/efi.img` -- готовий FAT32 ESP для USB/SD.

```
./build.sh aarch64
qemu-system-aarch64 -M virt -cpu cortex-a72 -m 1G -nographic \
    -kernel out/aarch64/Image -initrd out/aarch64/hext.img
qemu-system-aarch64 -M virt,acpi=off -cpu cortex-a72 -m 1G -nographic \
    -bios /usr/share/edk2/aarch64/QEMU_EFI.fd -drive file=out/aarch64/efi.img,format=raw,if=virtio
./build.sh riscv64
qemu-system-riscv64 -M virt -m 1G -nographic \
    -bios /usr/share/qemu/opensbi-riscv64-generic-fw_dynamic.bin \
    -kernel out/riscv64/kernel.elf -initrd out/riscv64/hext.img
```

Пристрої для повного робочого столу: `-device virtio-gpu-device
-device virtio-keyboard-device -device virtio-tablet-device -netdev
user,id=n0 -device virtio-net-device,netdev=n0` (або `-pci` варіанти).

Далі (поза планом): SMP на aarch64/riscv64 (PSCI `CPU_ON`, SBI HSM),
ACPI-машини без device tree, некогерентний DMA для реальних плат.
