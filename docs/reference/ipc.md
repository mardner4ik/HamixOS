# IPC HamixOS: поштові скриньки, служби, спільна пам'ять

Status: current · Updated: 2026-09-29 · План: `docs/roadmap/UNIX_PLAN.md` (U2)

Усе нижче -- звичайний POSIX (`AF_UNIX`, `SCM_CREDENTIALS`, `shm_open`), тож протокол
можна реалізувати на C, Python чи будь-чому іншому і на Linux/FreeBSD так само.
Rust-програми отримують це через `hxstd::sys::{msg_send, msg_recv, service_lookup,
shm_create, shm_map}`; приклад на C -- `tests/hxlibc/native-ipc.c`.

## Поштова скринька
* Кожен процес, що обмінюється повідомленнями, відкриває `socket(AF_UNIX, SOCK_DGRAM)`,
  вмикає `SO_PASSCRED` і прив'язує його до `/run/hamix/ipc/<pid>` (каталог `1777`).
* Повідомлення -- одна датаграма (до 64 КіБ). Відправник визначається лише з
  `SCM_CREDENTIALS` у `recvmsg` (pid, uid ставить ядро), а не з вмісту.
* Перед відправкою клієнт перевіряє, що власник сокета одержувача -- root або той самий
  uid, що в `/proc/<pid>/status`, щоб чужий процес не перехопив повідомлення.

## Реєстр служб
* Служба пише свій pid у файл: системні (root) -- `/run/hamix/services/<ім'я>`,
  користувацькі -- `$XDG_RUNTIME_DIR/hamix-services/<ім'я>` (каталог `0700`).
* Пошук: спершу `$XDG_RUNTIME_DIR/hamix-services`, потім `/run/hamix/services`; pid має
  бути живий (`kill(pid, 0)`), файл -- належати root або власному uid.
* Активація на вимогу: якщо служби немає, але є `/run/hamix/activators/<ім'я>` (pid hxsvc),
  клієнт надсилає туди `activate <ім'я>` і чекає до 5 с появи файла служби.
* Відомі імена: `hxserver` (Nook, hxproto), `hxwayland`, `hamix.auth` (hx-authd),
  керування hxsvc (`hxctl`).

## Спільна пам'ять
* Буфер створює власник: `shm_open("/hx-<id>", O_CREAT|O_EXCL|O_RDWR, 0600)`,
  `ftruncate`, `mmap(MAP_SHARED)`; `id = (pid << 12) | лічильник`. Одержувач відкриває те
  саме ім'я. Власник робить `shm_unlink` при звільненні; hxserver раз на 10 с прибирає
  `/dev/shm/hx-*` процесів, яких уже немає.
