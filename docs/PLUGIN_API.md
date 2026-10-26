# Lua API

Lua-плагин получает три глобальных значения: `alice`, `device` и `event`.
Аппаратные и системные операции требуют соответствующего permission в
`plugin.toml`; предоставляйте пакету только необходимые разрешения.

## Permissions

| Permission | Доступ |
| --- | --- |
| `led` | LED-кольцо и логотип |
| `volume` | Чтение и изменение громкости |
| `audio` | Воспроизведение аудиофайлов |
| `bluetooth` | Статус и управление Bluetooth |
| `display` | Подсветка дисплея |
| `sensors` | Перечень и чтение датчиков |
| `sensors.control` | Настройка датчиков |
| `clock` | Часы и их сегменты |
| `input` | Чтение разрешённых input-устройств |
| `wifi` | Состояние и управление Wi-Fi |
| `network` | Разрешённые HTTP-запросы |
| `ipc.alarm`, `ipc.speech` | События будильника и распознанной речи |
| `ipc.read`, `ipc.control` | Чтение и управление Quasar IPC |
| `system` | Сведения о системе |
| `exec` | Запуск разрешённых команд |

Сигнатуры и примеры всех функций приведены в
[каталоге Lua-функций](./functions/README.md). `alice.sleep(ms)` доступен без
permission и предназначен для пауз в циклах, чтобы плагин можно было отменить.

## Устройство и доступность функций

Таблица `device` содержит `id`, `model`, `architecture`, `led_count`, `features`
и boolean-словарь `capabilities`. Значения задаёт профиль устройства.
Плагины должны ориентироваться на возможности, а не на модель или конкретное
число LED:

```lua
if device.capabilities["led.ring.rgb"] then
    local colors = {}
    for i = 1, device.led_count do
        colors[i] = "#000000"
    end
    colors[1] = "#ff0000"
    alice.set(colors)
end
```

`features` сохраняет совместимость с `[compatibility].features` в манифесте.
Проверяйте capability конкретной операции: наличие `bluetooth.status`, например,
не означает, что доступны `bluetooth.power` или `bluetooth.scan`. Отсутствующая
capability считается `false`. Функции `is_available()` позволяют проверить
доступность без аппаратного вызова.

`alice.logo.set` и `alice.logo.off` — совместимые aliases LED-функций.
Количество цветов в `alice.set(colors)` и `alice.logo.set(colors)` определяется
через `device.led_count`.

## Совместимость пакета

В `[compatibility].features` указывайте необходимые возможности, а не модель:

```toml
[compatibility]
features = ["led.ring.rgb"]
```

Сведения о доступных возможностях текущих адаптеров приведены в
[матрице поддержки устройств](./DEVICE_SUPPORT.md).

## Сеть

Плагин может обращаться только к разрешённым публичным хостам. Для HTTPS
перечислите их в манифесте:

```toml
[network]
allow = ["api.example.org"]
```

HTTP разрешается только при явном `allow_insecure_http = true`. Loopback- и
private-адреса запрещены.

## Input

Плагин получает события только от устройств, перечисленных в
`[input].devices`. `alice.input.next_event(timeout_ms)` возвращает `nil` по
тайм-ауту либо таблицу `{device, type, code, value, time_ms}`.

```toml
permissions = ["input"]

[input]
devices = ["mute", "vad_keypad"]
```

## IPC

`alice.ipc.next_alarm(timeout_ms)` и
`alice.ipc.next_speech(timeout_ms)` возвращают `nil` либо событие. Текст
распознанной речи — персональные данные: обрабатывайте и передавайте его
только при явной необходимости.

Raw Quasar IPC (`next`, `send`, `request`) доступен при permissions
`ipc.read` и/или `ipc.control`. Вызовы ограничены allowlist сервисов:

```toml
permissions = ["ipc.read", "ipc.control"]

[ipc]
services = ["mediad"]
```

`ipc.read` открывает только `alice.ipc.next`; `ipc.control` — только
`alice.ipc.send`. `alice.ipc.request` требует оба permission и ждёт ответ с тем
же `request_id`, что и в protobuf field 1 исходного сообщения. Максимальный
тайм-аут — 60 секунд, размер IPC frame — 10 MiB. Allowlist обязателен и может
содержать не более 16 имён сервисов. `send` и `request` способны менять
состояние системных сервисов: выдавайте `ipc.control` только доверенным
пакетам.

На Yandex-устройствах daemon получает endpoint из `<service>.port` в
`/system/vendor/quasar/quasar.cfg`. При необходимости адрес можно задать через
`YAM_<SERVICE>_ADDR` в формате IPv4 loopback `host:port`.

Пример обмена бинарными сообщениями:

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
