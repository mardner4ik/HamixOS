# Бенчмарки: базова лінія

Status: current · Updated: 2026-09-27 · Інструменти: `cargo xtask kbench`, `cargo xtask membench`

Умови: QEMU/KVM, `-M q35 -m 1G`, 1 CPU, live-образ (`out/hamix_os.iso`).
Сирі дані -- `out/bench/*.json`, журнали -- `out/bench/*.log`.

## kbench (мкс на операцію; для read -- МіБ/с)

`tools/kbench/kbench.c` зібраний двічі: hxlibc (`hx-cc`, динамічний, Hamix-особистість) і
musl (static-pie, Linux-особистість) -- той самий код на тому самому ядрі.

| Тест | hxlibc | Linux/musl |
|---|---|---|
| getpid | 0.270 | 0.280 |
| clock_gettime | 0.320 | 0.270 |
| pipe ping-pong | 8.2 | 7.2 |
| fork + wait | 2200 | 1040 |
| fork + exec + wait | 6340 | 2470 |
| page fault (anon) | 0.98 | 0.73 |
| open + fstat + close | 5.3 | 4.9 |
| create file | 8.0 | 9.0 |
| read from cache | 8389 МіБ/с | 5592 МіБ/с |
| thread create + join | 30 | 24 |
| futex condvar ping-pong | 11.6 | 10.0 |

Висновки для «швидких виграшів» (§6.6 плану A, трек H):
* `fork` копіює адресний простір повністю; для динамічної hxlibc-програми (більше
  відображень) це вдвічі дорожче -- потрібен COW-fork і `vfork`/`posix_spawn` у hxlibc.
* `exec` динамічної програми читає `libhxc.so` (880 КБ) і `ld.hx` щоразу -- потрібен
  спільний page-cache для file-backed `mmap` (одна копія бібліотеки на всі процеси).
* `clock_gettime` -- справжній syscall; кандидат на vDSO.
* Роздільна здатність `CLOCK_MONOTONIC` -- 1 мс; `kbench` компенсує довгими циклами.

## membench (МіБ)

| Стан | used | kernel heap | vmalloc | page cache |
|---|---|---|---|---|
| консоль, 5 с після входу | 99 | 91 | 83 | 0 |
| Nook без вікон, 20 с | 112 | 94 | 86 | 0 |

83 МіБ vmalloc -- це live-корінь у RAM (образ hext). Ціль плану ("Nook <= 70 МБ")
міряється на встановленій системі; сценарій `membench --installed` -- наступний крок.

## Після copy-on-write fork (2026-09-28)

x86_64: `fork` більше не копіює сторінки -- спільні кадри позначаються COW (біт PTE,
лічильник посилань на кадр у `memory::frame`, розв'язання в обробнику #PF, увімкнений
`CR0.WP`), а копіювання таблиць виконується поза замком планувальника. Для процесу з
кількома потоками лишається повне копіювання (немає IPI TLB shootdown).

| Тест | hxlibc до | hxlibc після | musl після |
|---|---|---|---|
| fork + wait | 2200 | 990 | 965 |
| fork + exec + wait | 6340 | 4840 | 2380 |

Наступне для `exec`: спільний page-cache для file-backed `mmap` (`libhxc.so` читається
з VFS при кожному запуску динамічної програми).
