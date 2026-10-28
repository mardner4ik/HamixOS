# ADR-0010B: структура репозиторію і одна точка входу збірки

Status: accepted · Date: 2026-09-27 · Plan: §10.1

## Рішення
* Дерево: `kernel/`, `modules/` (було `drivers/`), `sdk/` (ABI модулів ядра і Hamix ABI),
  `hxlibc/` (форк mlibc), `libs/` (крейти ядра й інструментів хоста),
  `userland/{apps,core,desktop,services,libs}` (усі нативні програми й бібліотеки, і власні,
  і порти -- ADR-0015B), `layers/linux/` (увесь Linux-шар), `data/` (оверлей образу,
  GRUB), `tools/`, `tests/`, `docs/`, `out/`.
* Усе згенероване -- лише в `out/` (образи, ISO, rootfs, isoroot, логи тестів).
* Одна точка входу: `cargo xtask` (`build`, `run`, `test`, `lint-layers`, `abi`,
  `hxlibc-build`, `membench`, `kbench`), параметри `--arch` і `--profile`.
  `build.sh` -- обгортка для звички.
* `Cargo.lock` у git.
* Userland-програми знаходяться автоматично: кожен крейт з `src/main.rs` у
  `userland/*/*` -- нативний, у `layers/linux/compat/*` -- шару Linux; каталог із
  `HXBUILD` -- порт (`cargo xtask ports`).
* Кожен файл образу має джерело в дереві: базові файли `/etc` -- у `data/overlay`,
  файли шару -- у `layers/linux/overlay`.

## Наслідки
* Корінь чистий; `git status` не показує артефактів.
* Програма потрапляє в образ, щойно з'являється її каталог, без редагування списків.
