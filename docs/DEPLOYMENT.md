# Установка

Нужен `adb`. Установщик находит колонку, проверяет SHA-256 релизных файлов и ставит `yamd` в `/data/quazar/yam`.

```sh
curl -fsSL https://raw.githubusercontent.com/nyakokitsu/yam/main/install.sh | sh
```

Windows:

```powershell
irm https://raw.githubusercontent.com/nyakokitsu/yam/main/install.ps1 | iex
```

Для конкретной версии передайте её первым аргументом (`sh -s -- 0.1.13`). Удаление на Unix: `sh -s -- --uninstall`.

CLI обращается к `http://<колонка>:8099`; адрес можно переопределить `YAM_URL`.

## LED backend и профиль устройства

BetterYAM — plugin host и клиент стандартного Device API, не hardware daemon.
Физические адаптеры, discovery, LED count и capabilities реализуются отдельным
устройством daemon’ом. Daemon’ы реализуют один контракт: `GET /device`
возвращает JSON с `id`, `model`, `architecture`, `led_count` и `features`;
`POST /led` принимает `{"colors":["#RRGGBB", ...]}`, `POST /led/off`
выключает LED. BetterYAM не содержит sysfs LED-управления и не сопоставляет
частные endpoint’ы устройств.

URL стандартного Device API настраивается в `settings.toml` или Web UI:

```toml
[led]
backend = "http"

[led.http]
base_url = "http://device-daemon.example/"
```

Остальные аппаратные операции используют тот же Device API:

| Маршрут | Контракт |
| --- | --- |
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

JSON-ответы совместимы с типами Lua API; ошибки и неподдерживаемые
capabilities daemon сообщает HTTP status и телом ошибки.

`device.toml` может задавать профиль как резерв при недоступном daemon’е, но
hardware capabilities объявляются только по ответу `/device`, а не выводятся
из host sysfs. Для нового устройства добавляется отдельный daemon, а
BetterYAM и YAM plugins остаются неизменными. API-клиент передаёт frames без
предположений о числе LED; daemon проверяет фактический формат и возвращает
ошибки устройства.

## Безопасность и обновления

Web UI не имеет аутентификации: используйте только доверенную LAN. Автообновление включено; отключение в `settings.toml`:

```toml
[updates]
enabled = false
```
