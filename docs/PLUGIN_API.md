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
| `clock` | `alice.clock.available()`, `is_available()`, `time()`, `segments()`, `off()` |
| `input` | `alice.input.devices()`, `next_event(timeout_ms)` |
| `network` | `alice.http.get(url)`, `post(url, body)` |
| `ipc.alarm` | `alice.ipc.next_alarm(timeout_ms)` |
| `ipc.speech` | `alice.ipc.next_speech(timeout_ms)` |
| `system` | `alice.system.hostname()`, `uptime()`, `meminfo()` |
| `exec` | `alice.exec(program, args)` |

`alice.sleep(ms)` доступен всем пакетам и нужен в циклах для отмены запуска.

Для аппаратных API доступны boolean-проверки без выполнения операции:
`alice.led.is_available()`, `alice.led.ring.is_available()`,
`alice.led.ring.rgb.is_available()`, а также
`alice.volume.is_available()`, `alice.audio.is_available()`,
`alice.bluetooth.is_available()`, `alice.display.is_available()`,
`alice.sensors.is_available()`, `alice.clock.is_available()`,
`alice.input.is_available()`, `alice.sensors.control.is_available()`,
`alice.ipc.alarm.is_available()` и `alice.ipc.speech.is_available()`.
Для отдельных возможностей используйте `device.capabilities`, например
`device.capabilities["clock.segments"]` или
`device.capabilities["led.ring.12"]`. Capability keys включают API-группы и
отдельные операции, например `volume.set`, `bluetooth.scan`,
`display.backlight`, `sensors.ambient_light.configure`, `clock.time`,
`clock.segments` и `input.next_event`. Значение отсутствующей capability —
`false`. API сегментов часов остаётся вызываемым как раньше:
`alice.clock.segments({...})`; у него также есть
`alice.clock.segments.is_available()`.

`device` содержит `id`, `model`, `architecture`, `led_count`, `features` и
boolean-словарь `capabilities`, заполненные модельным BetterYAMd.
`features` сохраняет совместимость с `plugin.toml`. `alice.logo.set` и
`alice.logo.off` — совместимые aliases основных LED-функций. `alice.set`
использует `device.led_count`; проверку frame выполняет драйвер этой модели.

Например, текущий Yandex Mini backend сообщает `led.ring.rgb` и `led.ring.6`
как доступные; clock, volume, audio, Bluetooth, display, sensors, input и IPC
остаются в plugin API, но помечены недоступными до появления их драйверов.

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
