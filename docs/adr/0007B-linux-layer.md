# ADR-0007B: hxlinuxulator як ізольований шар, що вимикається однією дією

Status: accepted · Date: 2026-09-27 · Plan: §3, §6

## Контекст
Linux-сумісність (`kernel/src/syscall/linux`, `task/linux_loader.rs`, hxwayland,
`/opt/linux`, Alpine-ключі, GTK-тема Nook) була розкидана по дереву й образу, а
нативний код місцями знав про неї (`hamix_std::linux_bridge`, шляхи `/opt/linux` у hsh,
pantry, hxserver).

## Рішення
* **Правило листа:** залежності тільки Linux-шар -> нативне. Перевіряє
  `cargo xtask lint-layers` (Cargo/Meson-залежності, `linux_bridge`, шляхи `/opt/linux`).
* Userland-частина шару живе в `layers/linux/` (`ports/`, `overlay/`, `root/`, `compat/`;
  hxwayland з 2026-09-29 нативний -- ADR-0014B);
  ядерна -- у `kernel/src/syscall/linux` і відокремлена інтерфейсом
  `Personality` (`kernel/src/personality.rs`).
* **Особистість процесу** визначається при `execve` з ELF: нота `.note.hamix` ->
  Hamix; `PT_INTERP=/lib/ld-musl-*`, ET_DYN або статичний Linux-ELF -> Linux.
* **Перемикач:** runtime `hxctl linux off|on|status` (заборона нових Linux-процесів,
  завершення наявних); boot-параметр `linux=0`; профіль образу `hamix-core`
  (`cargo xtask build --profile core`) без жодного файлу шару.
* Linux-особистість ініціалізується ліниво -- при першому `execve` Linux-ELF.
* CI: образ `hamix-core` завантажується і проходить нативні boot-тести (`tests/boot`).

## Наслідки
* Нативне ніколи не ламається від вимкнення шару -- це перевіряється, а не обіцяється.
* Наступні кроки: розклад Alpine FHS замість `/opt/linux` (§6.4), `linux-base` пакунки,
  у 2027 -- шар як завантажуваний KPI-модуль.
