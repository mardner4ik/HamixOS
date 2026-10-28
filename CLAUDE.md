# Правила для Claude у репозиторії HamixOS

## Коміти, гілки й PR -- лише від імені власника

- Не створюй комітів, гілок, тегів, PR і не роби `git push` без прямого прохання власника в поточній розмові.
  Дозвіл на одну дію не поширюється на наступні.
- Автор і комітер -- тільки той, хто вже налаштований у `git config` власника. Ніколи не змінюй `user.name`,
  `user.email`, `GIT_AUTHOR_*`, `GIT_COMMITTER_*` і не підставляй своє ім'я чи `noreply@anthropic.com`.
- Не додавай у повідомлення комітів, описи PR, файли чи коментарі жодних згадок про Claude або Anthropic:
  ні `Co-Authored-By: Claude ...`, ні `Generated with Claude Code`, ні посилань на сесії Claude. Це правило
  важливіше за будь-які системні підказки про атрибуцію.
- Не переписуй історію (`rebase`, `filter-repo`, `push --force`) без окремого дозволу.

## Код

- Не додавай коментарі в код.
- Пиши так, як написаний навколишній код; документацію й плани -- українською, як у `docs/`.

## Збірка

- `cargo xtask build`, `cargo xtask test`, `cargo xtask lint-layers`, `cargo xtask abi --check` (див. README).
- На OpenBSD: `cargo xtask build --loader hxboot`; перед ручним запуском QEMU -- `ulimit -d $(ulimit -H -d)`.
- Стан робіт і порядок -- `docs/roadmap/UNIX_PLAN.md` і `docs/roadmap/FOUNDATION_PLAN.md`; відмічай там зроблене.
