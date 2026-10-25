# Контракт подключения общей Web Panel

Этот документ описывает точки подключения для общей панели BetterYAM. Панель
одна для всех устройств; модельные daemon предоставляют профиль, доступность
возможностей и HTTP API. Frontend не должен содержать ветвлений по модели или
управлять устройством напрямую.

## Основной принцип

1. При загрузке панели запросить `GET /api/device`.
2. Использовать `device.capabilities` для определения доступных разделов,
   полей и действий.
3. Показывать изображение, указанное профилем устройства.
4. Вызывать API только для доступных действий и отображать ошибки API
   пользователю.

Lua-функции вроде `alice.led.is_available()` и
`alice.led.ring.rgb.is_available()` предназначены для Lua-плагинов. Браузер
не запускает Lua и не вызывает эти функции: их эквивалент для панели —
булевы capability в ответе `/api/device`.

## Профиль устройства

Текущий `GET /api/device` возвращает:

```json
{
  "id": "yandexmini",
  "model": "YandexStationMini",
  "architecture": "armv7",
  "led_count": 6,
  "features": ["led.ring.rgb", "led.ring.6"],
  "capabilities": {
    "led.ring.rgb": true,
    "led.ring.6": true,
    "wifi": false
  },
  "source": "device profile"
}
```

`capabilities` — словарь `string -> boolean`. Проверять нужно конкретную
операцию, а не только родительскую группу. Например, раздел Bluetooth может
показывать статус при `bluetooth.status: true`, но включать управление питанием
только при `bluetooth.power: true`. Отсутствующая capability считается
недоступной. Не определяйте возможности по `model`, имени устройства,
`led_count` или наличию маршрута.

Для индивидуального изображения профилю нужно предоставить необязательное
поле `image` — same-origin URL к статическому изображению устройства, например:

```json
{
  "id": "yandexmini",
  "image": "/assets/devices/yandexmini.svg"
}
```

Это поле пока отсутствует в текущем `DeviceProfile`; чтобы подключить
изображения через профиль, backend должен добавить его в `GET /api/device`
и раздавать соответствующие assets. Общая панель отображает `image`, если оно
есть, и использует нейтральное изображение-заполнитель, если нет. Не добавляйте
в общий frontend таблицы вида `if (device.id === "...")`.

## HTTP API панели

Все пути ниже включают префикс `/api`. Запросы с JSON-телом отправляются с
`Content-Type: application/json`. Для успешного ответа без тела daemon может
вернуть `204 No Content`. При ошибке возвращается HTTP error status; текст
ошибки находится в поле `error` JSON-ответа. Панель должна показывать эту
ошибку, а не сообщать об успехе.

| Раздел / capability | Метод и путь | Контракт |
| --- | --- | --- |
| Устройство | `GET /api/device` | Device profile, features и capabilities |
| Пакеты | `GET /api/plugins` | Установленные плагины и их состояние |
| Пакеты | `POST /api/plugins` | Установка multipart-поля `package` |
| Пакет | `DELETE /api/plugins/{id}` | Удаление |
| Пакет | `POST /api/plugins/{id}/toggle` | Включить/выключить |
| Пакет | `POST /api/plugins/{id}/invoke` | Запустить/перезапустить |
| Пакет | `POST /api/plugins/{id}/stop` | Остановить |
| Пакет | `GET /api/plugins/{id}/logs` | Журнал |
| Настройки | `GET /api/settings`, `PUT /api/settings` | Чтение и сохранение настроек runtime и Wi-Fi setup |
| LED | `POST /api/led` | `{ "colors": ["#RRGGBB", "..."] }`; число цветов определяется профилем |
| LED | `POST /api/led/off` | Выключить LED |
| Логотип | `POST /api/logo` | Та же форма кадра, что и `/api/led` |
| Логотип | `POST /api/logo/off` | Выключить логотип |
| Датчики (`sensors`) | `GET /api/sensors` | Список датчиков |
| Датчик (`sensors.read`) | `GET /api/sensors/{sensor}` | Значение выбранного датчика |
| Часы (`clock`) | `GET /api/clock`, `PUT /api/clock` | Состояние и управление; тело может содержать `time`, `brightness`, `segments` или `off` |
| Громкость (`volume.get`, `volume.set`) | `GET /api/volume`, `PUT /api/volume` | Установка: `{ "percent": 0..100 }` |
| Bluetooth (`bluetooth.status`) | `GET /api/bluetooth/status` | Статус Bluetooth |
| Bluetooth (`bluetooth.power`) | `PUT /api/bluetooth/power` | `{ "on": true }` или `{ "on": false }` |
| Wi-Fi (`wifi.status`) | `GET /api/wifi` | Состояние соединения |
| Wi-Fi (`wifi.networks`) | `GET /api/wifi/networks` | Сохранённые и обнаруженные сети; пароли не возвращаются |
| Wi-Fi (`wifi.scan`) | `POST /api/wifi/scan` | Запустить сканирование |
| Wi-Fi (`wifi.networks`) | `POST /api/wifi/networks` | `{ "ssid": "...", "password": "..." }` |
| Wi-Fi (`wifi.networks`) | `DELETE /api/wifi/networks/{ssid}` | Удалить сохранённую сеть |
| Wi-Fi setup (`wifi.ap`) | `GET /api/wifi/setup`, `POST /api/wifi/setup` | Статус и управление; запрос `{ "on": true }` или `{ "on": false }` |

Маршруты runtime-плагинов `/api/runtime/{id}/...` имеют отдельную модель
доступа и токены. Они не являются API управления панелью и не должны
использоваться для действий пользователя.

## Сопоставление UI и capabilities

Панель строит интерфейс из точных ключей профиля. Примеры:

| UI-функция | Capability |
| --- | --- |
| Показать управление кольцом | `led.ring.rgb` |
| Показать действие выключения LED | `led.off` |
| Выбрать число сегментов / цветов | `led.ring.<count>` и `led_count` |
| Показать часы | `clock`; отдельные операции — `clock.time`, `clock.segments`, `clock.off` |
| Показать уровень громкости | `volume.get`; управление — `volume.set` |
| Показать Bluetooth status / power | `bluetooth.status` / `bluetooth.power` |
| Показать Wi-Fi status, scan, saved networks, AP setup | `wifi.status`, `wifi.scan`, `wifi.networks`, `wifi.ap` |
| Показать датчики / чтение / настройку | `sensors.list`, `sensors.read`, `sensors.control` |

Capability описывает поддержку операции устройством, а не её текущее состояние.
Например, `wifi.ap: true` означает, что backend поддерживает управление точкой
доступа; включена ли она сейчас, нужно узнавать через `GET /api/wifi/setup`.
Если действие недоступно, его следует скрыть или отключить с пояснением, а не
отправлять заведомо неподдерживаемый запрос.

## Точки подключения в репозитории

- `yandexmini/web/index.html` — текущая встроенная панель и пример вызовов API.
- `yandexmini/main.rs` — HTTP routes, `DeviceProfile` и обработчики API.
- `yandexmini/led.rs` — профиль capabilities, публикуемый Mini backend.
- `docs/DEPLOYMENT.md` — описание общего Device API и capabilities.

Сейчас HTML панели находится внутри каталога Mini. При переходе на общий
frontend его исходник и assets должны стать общими для daemon-ов и встраиваться
при сборке каждого устройства; модельные backend должны предоставлять только
профиль, capabilities, API реализацию и собственное изображение.
