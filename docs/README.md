# Документація HamixOS

Статус: `current` -- описує систему як є; `draft` -- рішення прийняте, робота йде;
`historical` -- лишається для історії, не є основою; `plan` -- план/стратегія.

## Рішення (ADR)

| ADR | Тема | Статус |
|---|---|---|
| [0005B](adr/0005B-hxlibc-mlibc.md) | hxlibc -- власна libc як форк mlibc | draft |
| [0006B](adr/0006B-hamix-abi-v1.md) | Hamix ABI v1 | current |
| [0007B](adr/0007B-linux-layer.md) | Linux-шар, що вимикається однією дією | current |
| [0008B](adr/0008B-hxproto-v2.md) | hxserver і hxproto v2 замість Wayland | draft |
| [0009B](adr/0009B-hxsvc-init.md) | hxsvc як PID 1 | current |
| [0010B](adr/0010B-repository-layout.md) | структура репозиторію, `cargo xtask` | current |
| [0011B](adr/0011B-gpu-class-drm-compat.md) | нативний GPU-клас, `/dev/dri` у Linux-шарі | accepted, крок 1 (dumb KMS) |
| [0012B](adr/0012B-bash-system-shell.md) | bash на hxlibc -- системна оболонка, команди -- окремі програми | current |
| [0013B](adr/0013B-linux-environment-and-live-root.md) | Linux-оточення в `/linux`, живий корінь із диска | current |
| [0014B](adr/0014B-native-gtk-and-hxwayland.md) | GTK 3 на hxlibc, нативний hxwayland | current |
| [0015B](adr/0015B-userland-layout-and-hxlibc-everywhere.md) | усі програми в `userland/<роль>/`, Rust-програми на hxlibc | current |
| [0016B](adr/0016B-mesa-softpipe-glvnd.md) | Mesa (softpipe) + libglvnd на hxlibc | current |
| [0017B](adr/0017B-ports-tree-and-c-libraries.md) | `ports/` окремо від `userland/`, C-бібліотеки (`libhxproto`) | current |
| [0018B](adr/0018B-redox-style-layering.md) | шари як у Redox: ядро за підсистемами, `crates/`, повне дерево hxlibc, initfs | current |

## Довідники

| Документ | Статус |
|---|---|
| [reference/hamix-abi.md](reference/hamix-abi.md) -- нативні системні виклики, ELF-нота, ревізії | current |
| [reference/hxlibc-status.md](reference/hxlibc-status.md) -- що збирається на hxlibc | draft |

## Архітектура

| Документ | Статус |
|---|---|
| [FILESYSTEM](architecture/FILESYSTEM.md) -- розкладка системи, Linux-оточення `/linux` | current |
| [HEXT](architecture/HEXT.md) -- файлова система hext | current |
| [HXINIT](architecture/HXINIT.md) -- ядерний етап завантаження | current |
| [LINUXULATOR](architecture/LINUXULATOR.md) -- Linux-особистість | current (розклад `/opt/linux` замінено спільним деревом, ADR-0007B) |
| [MODULES](architecture/MODULES.md) -- модулі драйверів і KPI | current |
| [NETWORK](architecture/NETWORK.md) · [AUDIO](architecture/AUDIO.md) · [VIDEO](architecture/VIDEO.md) | current |
| [XORG](architecture/XORG.md) -- hxserver і hxproto | current |
| [PERMISSIONS](architecture/PERMISSIONS.md) -- користувачі, sudo | current |
| [MUSL](architecture/MUSL.md) -- musl у Linux-шарі | current |
| [OPTIMIZATION](architecture/OPTIMIZATION.md) -- план оптимізації | plan |

## Посібники

| Документ | Статус |
|---|---|
| [INSTALL](guides/INSTALL.md) · [SHELL](guides/SHELL.md) · [COMMANDS](guides/COMMANDS.md) | current |
| [HSH](guides/HSH.md) -- колишня оболонка, вимкнена (ADR-0012B) | historical |
| [PANTRY](guides/PANTRY.md) -- пакунки, шари `native`/`linux` | current |
| [IPC](reference/ipc.md) -- поштові скриньки, служби, спільна пам'ять | current |
| [PORTS](guides/PORTS.md) -- стороннє ПЗ на hxlibc (`HXBUILD`) | current |
| [PORTING](guides/PORTING.md) -- порти aarch64/riscv64 | current |
| [SERVICES](guides/SERVICES.md) -- hxsvc, `.svc`-файли, hxctl | current |
| [BUILDING](guides/BUILDING.md) -- `cargo xtask`, профілі, тести | current |

## Плани

| Документ | Статус |
|---|---|
| [STRATEGY_2026_B](roadmap/STRATEGY_2026_B.md) -- обраний план | plan |
| [STRATEGY_2026](roadmap/STRATEGY_2026.md) -- альтернатива A | historical |
| [ARCHITECTURE_ROADMAP](roadmap/ARCHITECTURE_ROADMAP.md) · [GRAPHICS_ROADMAP](roadmap/GRAPHICS_ROADMAP.md) · [KPI_ROADMAP](roadmap/KPI_ROADMAP.md) | plan |
| [USERSPACE_ROADMAP](history/USERSPACE_ROADMAP.md) | historical |
