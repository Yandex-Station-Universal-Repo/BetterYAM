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
| `wifi` | `alice.wifi.status()`, `scan()`, `add(ssid, password?)`, `forget(ssid)`, `setup(on)` |
| `network` | `alice.http.get(url)`, `post(url, body)` |
| `ipc.alarm` | `alice.ipc.next_alarm(timeout_ms)` |
| `ipc.speech` | `alice.ipc.next_speech(timeout_ms)` |
| `ipc.read` | `alice.ipc.next(service, timeout_ms)` |
| `ipc.control` | `alice.ipc.send(service, message)`; вместе с `ipc.read` — `alice.ipc.request(service, message, timeout_ms)` |
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
`alice.wifi.is_available()` сообщает, доступен ли Wi-Fi adapter.
Для отдельных возможностей используйте `device.capabilities`, например
`device.capabilities["clock.segments"]` или
`device.capabilities["led.ring.12"]`. Capability keys включают API-группы и
отдельные операции, например `volume.set`, `bluetooth.scan`,
`display.backlight`, `sensors.ambient_light.configure`, `clock.time`,
`clock.segments` и `input.next_event`. Значение отсутствующей capability —
`false`. API сегментов часов остаётся вызываемым как раньше:
`alice.clock.segments({...})`; у него также есть
`alice.clock.segments.is_available()`.

Permission `wifi` предоставляет доступ к сетям устройства и поэтому позволяет
плагину менять сохранённые подключения или включать точку доступа. Не выдавайте
его пакетам, которым эти операции не нужны. `alice.wifi.status()` возвращает
текущую сеть, сохранённые и обнаруженные сети, а также состояние точки доступа;
пароли в ответ не включаются. `scan()` возвращает `false`, если устройство не
может сканировать в текущем режиме.

`device` содержит `id`, `model`, `architecture`, `led_count`, `features` и
boolean-словарь `capabilities`, заполненные модельным BetterYAMd.
`features` сохраняет совместимость с `plugin.toml`. `alice.logo.set` и
`alice.logo.off` — совместимые aliases основных LED-функций. `alice.set`
использует `device.led_count`; проверку frame выполняет драйвер этой модели.

Подробные сигнатуры, permissions, результаты и примеры приведены в общем
[каталоге функций](./functions/README.md). Это контракт runtime, а не обещание,
что каждая операция реализована на каждом устройстве. Фактическая доступность
определяется boolean capabilities из профиля `device`; смотрите
[матрицу поддержки адаптеров](./DEVICE_SUPPORT.md).

Например, LED API принимает число цветов из `device.led_count`; профиль, а не
Lua-код плагина, задаёт hardware-specific параметры. Датчики, input, volume,
audio, Bluetooth и Wi-Fi объявляются доступными только при обнаружении их
backend. Capability отдельной операции может быть `false`, даже если соседние
операции той же группы доступны.

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

Raw Quasar IPC работает с бинарными protobuf `QuasarMessage`: `next` возвращает
Lua binary string или `nil` по тайм-ауту, `send` передаёт binary string сервису,
а `request` ждёт ответ с тем же `request_id` из protobuf field 1. Максимальный
timeout — 60 секунд; максимальный размер одного IPC frame — 10 MiB.

Разрешайте конкретные сервисы в `[ipc].services`; вызовы к другим сервисам
отклоняются:

```toml
permissions = ["ipc.read", "ipc.control"]

[ipc]
services = ["mediad"]
```

`ipc.read` открывает только `alice.ipc.next`; `ipc.control` открывает только
`alice.ipc.send`. `alice.ipc.request` существует только при наличии обоих
permissions. `ipc.read` и `ipc.control` требуют непустой allowlist; максимум
16 имён сервисов. `send` и `request` позволяют менять состояние system
services — выдавайте `ipc.control` только доверенным пакетам и перечисляйте
минимально необходимые сервисы. На Yandex устройствах daemon находит endpoint
по `<service>.port` в `/system/vendor/quasar/quasar.cfg`; для настройки адреса
можно задать `YAM_<SERVICE>_ADDR` как IPv4 loopback `host:port`.

Пример приёма сообщения и отправки запроса (protobuf bytes должны быть
сформированы вашим Lua-кодом или библиотекой protobuf):

```lua
local frame = alice.ipc.next("mediad", 1000)
if frame then
    -- frame is a binary Lua string, not JSON or UTF-8 text.
end

alice.ipc.send("mediad", protobuf_bytes)
local response = alice.ipc.request(
    "mediad",
    request_with_request_id,
    5000
)
```
