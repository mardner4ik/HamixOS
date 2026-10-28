# Сервіси: hxsvc і hxctl

Status: current · Updated: 2026-09-27 · ADR: `docs/adr/0009B-hxsvc-init.md`

Після ядерного етапу `hxinit` ядро запускає `/usr/libexec/hamix/hxsvc` як **PID 1**
(параметр ядра `init=ШЛЯХ`; `init=kernel` повертає старий ядерний логін).
`hxsvc` читає `/etc/hamix/services/*.svc`, запускає сервіси, збирає їхній вивід у
журнал і перезапускає за політикою.

## Формат `.svc`

```
description = Login prompt on a text console
exec = /usr/libexec/hamix/hxlogin --tty %i
start = vt
restart = always
```

| Ключ | Значення |
|---|---|
| `exec` | команда; `%i` -- параметр екземпляра для шаблонів `ім'я@.svc` |
| `start` | `boot` (при старті), `manual`, `vt` (екземпляр на кожну активовану консоль), `demand` (ледачий старт за сервіс-портом) |
| `restart` | `no`, `on-failure`, `always` (не частіше 5 разів за 10 с) |
| `after` | імена сервісів, які стартують раніше |
| `user` | від чийого імені запускати (типово root) |
| `memory_max` | ліміт у КіБ (або `64M`); перевищення -> завершення й політика `restart` |
| `provides` | імена сервіс-портів для `start = demand` |
| `layer` | `native` (типово) або `linux`: такі сервіси зупиняються при `hxctl linux off` |
| `console` | `yes` -- сервіс працює на консолі без перенаправлення виводу |

Ледачий старт: для `start = demand` `hxsvc` реєструє себе активатором імен з
`provides` (`SERVICE_ACTIVATOR`). Перший `SERVICE_LOOKUP` такого імені будить
`hxsvc`, той запускає сервіс, і клієнт отримує його pid, щойно сервіс
зареєструє порт (до 5 с).

Консолі: на tty1 логін стартує одразу; на tty2..tty6 -- коли на неї вперше
перемикаються (ядро надсилає PID 1 повідомлення `vt N`).

## Журнал

Вивід кожного сервісу -- у `/var/log/hx/ІМ'Я.log` і в кільцевий буфер (200 рядків).
Події самого супервізора -- у `/var/log/hx/hxsvc.log`.

## hxctl

```
hxctl status [СЕРВІС]
hxctl start|stop|restart СЕРВІС
hxctl log [СЕРВІС] [РЯДКІВ]
hxctl reload
hxctl linux on|off|status
hxctl abi
hxctl poweroff|reboot
```

Команди, що змінюють стан, потребують root. `hxctl linux off` забороняє нові
Linux-процеси, завершує наявні й зупиняє сервіси з `layer = linux`.

## Адаптери Linux-шару

Усі вони -- сервіси з `layer = linux`: зникають разом із шаром.

* **hxsvc-openrc** (`/usr/libexec/hamix/hxsvc-openrc`). Для кожного скрипта в
  `/etc/init.d` створює юніт `openrc-ІМ'Я.svc` (`start = manual`). Запускається
  під час завантаження (`hxsvc-openrc-sync.svc`) і хуком pantry
  `/etc/pantry/hooks.d/50-openrc` після встановлення пакунків.
  `hxctl start openrc-dbus` запускає OpenRC-скрипт, і hxsvc стежить за ним.
* **hx-login1** (`/usr/libexec/hamix/hx-login1`, юніт `hx-login1.svc`) --
  `org.freedesktop.login1` на системній шині D-Bus для Linux-програм. Чекає,
  поки з'явиться `/run/dbus/system_bus_socket`, і займає ім'я (політика --
  `/usr/share/dbus-1/system.d/org.freedesktop.login1.conf`).
  Реалізовано: `Manager` (`GetSession`, `GetSessionByPID`, `GetUser`,
  `GetUserByPID`, `GetSeat`, `ListSessions`, `ListUsers`, `ListSeats`,
  `ListInhibitors`, `Can*`, `PowerOff`/`Reboot` через `hxctl`, `Inhibit`
  повертає дескриптор), `Session`, `Seat`, `User`, `Properties.Get/GetAll` та
  `Introspect`. Сесія -- одна на кожного користувача з uid >= 1000, у якого є
  процеси; `Suspend`/`Hibernate` і `TakeControl`/`TakeDevice` відповідають
  `NotSupported`, бо пристроями керує нативний дисплей.
* **hx-upower** (`/usr/libexec/hamix/hx-upower`, юніт `hx-upower.svc`) -- `org.freedesktop.UPower`:
  `EnumerateDevices` (адаптер `line_power_AC`), `GetDisplayDevice`, `GetCriticalAction`,
  властивості демона й пристроїв. Ядро поки не читає стан батареї (немає ACPI AML),
  тому шим каже "живлення від мережі, батареї немає" -- цього досить, щоб GTK/XFCE не
  показували помилок живлення.
* **hx-polkit** (`/usr/libexec/hamix/hx-polkit`, юніт `hx-polkit.svc`) --
  `org.freedesktop.PolicyKit1.Authority`: `CheckAuthorization` для суб'єктів
  `unix-process`, `system-bus-name` (uid через `GetConnectionUnixUser`) і `unix-session`.
  Політика: root -- дозволено; дії для активної сесії (`login1.power-off/reboot/suspend`,
  `udisks2.filesystem-mount`, `upower.*` ...) -- дозволено; інше для користувачів із
  `/etc/sudoers` -- `is_challenge` (агента автентифікації ще немає, працює `sudo`), для
  решти -- відмова. Реєстрація агентів приймається без дії.
* Спільний D-Bus-код адаптерів -- крейт `layers/linux/compat/hxdbus` (протокол, `Properties`,
  `Introspect`, цикл обслуговування з перепідключенням). Системна шина --
  нативний `dbus-daemon` з hx-ports (користувач `messagebus` є в базі).
* **libudev-zero**. `/etc/pantry/prefer` каже pantry брати `libudev-zero`,
  коли пакунку потрібен `so:libudev.so.1` (без демона udev). Перевірити вибір:
  `pantry provider so:libudev.so.1`.
