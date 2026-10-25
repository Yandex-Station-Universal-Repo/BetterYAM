# Установка

Нужен `adb`. Установщик определяет имя модели через `uname -n`, загружает CLI
и соответствующий этой модели BetterYAMd, проверяет SHA-256 и устанавливает
демон в `/data/quazar/betteryam`.

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/main/install.sh | sh
```

Windows:

```powershell
irm https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/main/install.ps1 | iex
```

Для конкретной версии передайте её первым аргументом (`sh -s -- 0.1.13`). Удаление на Unix: `sh -s -- --uninstall`.

CLI обращается к `http://<колонка>:8099`; адрес можно переопределить `YAM_URL`.
Имена device daemon release assets имеют вид
`betteryamd-<model-id>-aarch64-linux`. `<model-id>` — hostname в lower case,
символы вне `[a-z0-9_-]` заменяются дефисами. Asset и его checksum должны
присутствовать в `SHA256SUMS` релиза. Для неподдерживаемой модели installer
завершается с ошибкой, не устанавливая другой model daemon.

## BetterYAMd и Device API

CLI и Web Panel — HTTP-клиенты. BetterYAMd реализует YAM API и связывает его с
hardware конкретной модели; для каждой модели публикуется отдельный asset.
Модельные реализации используют общий Device API контракт, сохраняя полный
набор методов из таблицы ниже. Общий plugin API не содержит model-specific
условий и прямого управления hardware.

`GET /device` возвращает `id`, `model`, `architecture`, `led_count`, `features`
и словарь `capabilities` с boolean значениями. `features` сохраняется для
совместимости с manifest compatibility; `capabilities` отвечает за доступность
каждой отдельной операции/возможности. Отсутствующий capability считается
недоступным (`false`). Например:

```json
{
  "features": ["led.ring.rgb", "led.ring.12", "clock"],
  "capabilities": {
    "led.ring.rgb": true,
    "led.ring.12": true,
    "clock": true,
    "clock.segments": false
  }
}
```

Для режима, в котором plugin host обращается к отдельной реализации Device API,
адрес задаётся в `settings.toml` или Web UI:

```toml
[led]
backend = "http"

[led.http]
base_url = "http://device-daemon.example/"
```

Остальные аппаратные операции используют тот же Device API:

| Маршрут | Контракт |
| --- | --- |
| `GET /device` | Профиль, feature list и булевы capabilities |
| `POST /led`, `POST /led/off` | `{colors: ["#RRGGBB", ...]}` / выключить LED |
| `GET /sensors`, `GET /sensors/{id}` | Список `{id, kind, unit, value}` и чтение сенсора |
| `POST /sensors/ambient-light/configure` | `{name, gain, integration_ms}` |
| `GET /clock`, `PUT /clock` | `ClockInfo`; запрос принимает `time`, `brightness`, `segments` или `off` |
| `GET /volume`, `PUT /volume` | Получение уровня; установка `{percent}` |
| `POST /audio/play` | Тело запроса — audio bytes из файла пакета |
| `GET /bluetooth/status` | JSON-статус |
| `PUT /bluetooth/power`, `PUT /bluetooth/scan` | `{on: boolean}` |
| `POST /bluetooth/pair`, `/connect`, `/disconnect`, `/remove` | `{address: MAC}` |
| `GET /display/backlights` | Список доступных backlight IDs |
| `PUT /display/backlight` | `{name, value}` |
| `GET /input/devices` | Список доступных input device names |
| `POST /input/next-event` | `{devices, timeout_ms}`, ответ — event или `null` |
| `POST /ipc/alarm/next`, `/ipc/speech/next` | `{timeout_ms}`, ответ — event или `null` |

JSON-ответы совместимы с типами Lua API; ошибки и неподдерживаемые операции
возвращаются с HTTP status и телом ошибки. Lua сохраняет прежние методы, а
`alice.<api>.is_available()` и `device.capabilities["<capability>"]` позволяют
проверить доступность без вызова операции. Для сегментов часов сохраняется
вызов `alice.clock.segments({...})`; теперь этот callable API также предоставляет
`alice.clock.segments.is_available()`.

`device.toml` может задавать базовый профиль при недоступности Device API, но
hardware capabilities не выводятся из host sysfs. LED frames передаются без
предположений о числе светодиодов; model daemon проверяет формат для своего
устройства и сообщает ошибки.

## Безопасность и обновления

Web UI не имеет аутентификации: используйте только доверенную LAN. Обновление BetterYAMd выполняется установщиком: он выбирает asset по текущей
модели станции.
