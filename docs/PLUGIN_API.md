# Lua API

Lua-пакет получает `alice`, `device` и `event`. Укажите минимальный список `permissions` в `plugin.toml`; API без разрешения недоступен.

| Permission | API |
| --- | --- |
| `led` | `alice.set(colors)`, `off()`, `play(path)`, `logo.set(colors)`, `logo.off()` |
| `volume` | `alice.volume.get()`, `set(percent)` |
| `audio` | `alice.audio.play(path)` |
| `bluetooth` | `alice.bluetooth.status()`, `power()`, `scan()`, `pair()`, `connect()`, `disconnect()`, `remove()` |
| `display` | `alice.display.backlights()`, `set_backlight()` |
| `sensors` | `alice.sensors.list()`, `read(id)` |
| `sensors.control` | `alice.sensors.configure_ambient_light()` |
| `clock` | `alice.clock.available()`, `time()`, `segments()`, `off()` |
| `input` | `alice.input.devices()`, `next_event(timeout_ms)` |
| `network` | `alice.http.get(url)`, `post(url, body)` |
| `ipc.alarm` | `alice.ipc.next_alarm(timeout_ms)` |
| `ipc.speech` | `alice.ipc.next_speech(timeout_ms)` |
| `system` | `alice.system.hostname()`, `uptime()`, `meminfo()` |
| `exec` | `alice.exec(program, args)` |

`alice.sleep(ms)` доступен всем пакетам и нужен в циклах для отмены запуска.

## Совместимость и сеть

Указывайте нужные возможности, а не модель:

```toml
[compatibility]
features = ["led.ring.rgb"]

[network]
allow = ["api.example.org"]
```

`network` допускает HTTPS только к объявленным публичным хостам. Для HTTP добавьте `allow_insecure_http = true`. Локальные и private-адреса запрещены.

## Input и IPC

Для `alice.input.next_event` перечислите exact kernel device names; прочие `/dev/input` пакет не получает:

```toml
permissions = ["input"]
[input]
devices = ["mute", "vad_keypad"]
```

Функция возвращает `nil` по тайм-ауту либо `{ device, type, code, value, time_ms }`. `alice.ipc.next_alarm` и `next_speech` возвращают `nil` либо событие будильника/распознанной речи. Текст речи — персональные данные: не отправляйте его без явной необходимости и permission `network`.
