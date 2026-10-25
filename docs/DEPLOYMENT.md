# Установка

Нужен `adb`. Установщик определяет имя модели через `uname -n`, ищет точную
запись в release `manifest.json`, проверяет SHA-256 и устанавливает выбранный
BetterYAMd в `/data/quazar/betteryam`.

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/main/install.sh | sh
```

Windows:

```powershell
irm https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/main/install.ps1 | iex
```

Для конкретной версии передайте её первым аргументом (`sh -s -- 0.1.13`). Удаление на Unix: `sh -s -- --uninstall`.

CLI обращается к `http://<колонка>:8099`; адрес можно переопределить `YAM_URL`.
Каталог сборки — root `manifest.json`. Запустите
`sh tools/build-daemons.sh`; скрипт собирает все перечисленные устройства и
создаёт `dist/manifest.json`, daemon assets в `dist/assets/` и `dist/SHA256SUMS`
для всех файлов в `dist/assets/`. CLI release assets, которые собираются
отдельно, также нужно положить в `dist/assets/` перед сборкой, чтобы их
checksum вошёл в общий файл. Загрузите manifest, `SHA256SUMS` и все assets как
release assets. Неизвестная модель не сопоставляется с похожим устройством:
установка завершается с ошибкой.

## YAM API и capability profile

CLI подключается к HTTP API BetterYAMd. Сервер API и Web Panel принадлежат
папке соответствующей модели; модельный backend управляет своим hardware
напрямую. Yandex Mini реализован в `yandexmini/`: `main.rs` содержит API и
plugin runtime, `led.rs` — аппаратную функцию LED.

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

Полный контракт API сохраняется:

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

В текущем Mini backend реализован LED ring (`led.ring.rgb` и
`led.ring.6`). Остальные перечисленные операции/API оставлены в runtime, но
Mini сообщает для них `capabilities: false` и возвращает понятную ошибку при
вызове до появления hardware-реализации. Lua сохраняет прежние методы, а
`alice.<api>.is_available()` и `device.capabilities["<capability>"]` позволяют
проверить доступность без вызова операции. Для сегментов часов сохраняется
вызов `alice.clock.segments({...})`; теперь этот callable API также предоставляет
`alice.clock.segments.is_available()`.

LED frames для Mini должны содержать ровно шесть цветов. Аппаратный драйвер
останавливает запуск, если `/dev/i2c-0` или LED controller недоступны.

## Безопасность и обновления

Web UI не имеет аутентификации: используйте только доверенную LAN. Обновление BetterYAMd выполняется установщиком: он выбирает asset по текущей
модели станции.
