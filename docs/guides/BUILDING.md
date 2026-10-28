# Збірка і тести

Status: current · Updated: 2026-09-27 · ADR: `docs/adr/0010B-repository-layout.md`

Одна точка входу -- `cargo xtask` (крейт `tools/xtask`, збирається під хост).

| Команда | Що робить |
|---|---|
| `cargo xtask build [--arch A] [--profile P] [--extra DIR]` | ядро, userland, модулі, образ hext і (x86_64) ISO у `out/` |
| `cargo xtask kernel` | лише ядро |
| `cargo xtask run [--profile P] [-- QEMU-ARGS]` | запуск останнього образу в QEMU |
| `cargo xtask test [--profile P] [--only NAME] [--net]` | завантажувальні тести `tests/boot/*.sh` |
| `cargo xtask lint-layers` | правило листа: нативне не залежить від `layers/linux` |
| `cargo xtask abi [--check]` | генерує таблицю ядра, `hamix/syscall.h` і `hxlibc-sys` з `docs/reference/hamix-abi.md` |
| `cargo xtask hxlibc-build` | збирає hxlibc (mlibc) і sysroot для `hx-cc` |
| `cargo xtask membench` · `cargo xtask kbench` | пам'ять і швидкість ядра в QEMU, JSON у `out/bench` |

Профілі: `full` (`out/hamix_os.iso`, нативна система + Linux-шар) і `core`
(`out/hamix_core.iso`, без жодного файлу Linux-шару, ядро з `linux=0`).

## Завантажувальні тести

Кожен `tests/boot/*.sh` -- скрипт bash. Тест завантажує ISO з параметрами
`console=ttyS0 hxtest=/usr/lib/hamix/tests/NAME.sh`; `hxsvc` виконує скрипт через
`/usr/bin/bash`, друкує `hxtest: exit N` і вимикає машину. Тест провалено, якщо код не 0
або в журналі є рядок `FAIL...`, `...: command not found`, `syscall: unimplemented`,
`panicked at`.

Префікси: `linux-*` -- лише профіль `full`, `core-*` -- лише `core`,
`*-net-*` -- лише з `--net` (потрібен інтернет). Журнали -- `out/test/NAME.log`.

## Куди що класти

* нова нативна програма -- `userland/{core,services,desktop,apps}/ІМ'Я` (крейт із
  `src/main.rs`; потрапляє в образ автоматично); крейт може мати кілька програм у
  `src/bin/*.rs` -- кожна стає окремим файлом `/usr/bin/ІМ'Я` (так зроблено `hxutils`);
* нова команда оболонки -- файл `userland/core/hxutils/src/bin/ІМ'Я.rs`, не hsh;
* метадані крейта: `[package.metadata.hamix]` з `disabled = true` (крейт лишається в
  дереві, але не збирається в образ -- так вимкнено `hsh`) або
  `aliases = ["mkfs.hext=mkfs", "/bin/sh=sh"]` (копії програми під іншими іменами);
* частина Linux-шару -- `layers/linux/...`;
* файл образу -- `data/overlay/...` (нативний) або `layers/linux/overlay/...` (шар);
* новий нативний виклик -- спершу рядок у `docs/reference/hamix-abi.md`, потім
  `cargo xtask abi`, потім реалізація в `kernel/src/syscall/ext`.
