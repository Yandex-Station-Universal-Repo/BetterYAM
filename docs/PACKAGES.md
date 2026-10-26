# Пакеты и CLI

## Формат пакета

Пакет `.yap` — ZIP- или `tar.gz`-архив с `plugin.toml` в корне. Пути внутри
пакета должны быть относительными.

```toml
id = "ambient-lights"
name = "Ambient lights"
version = "1.0.0"
entry = "main.lua"
permissions = ["led"]

[launch]
kind = "lua"       # lua | binary
mode = "service"   # oneshot | service
path = "main.lua"

[compatibility]
features = ["led.ring.rgb"]
```

Указывайте только необходимые `permissions` и аппаратные возможности в
`[compatibility].features`. Контракт Lua API описан в
[документации плагинов](./PLUGIN_API.md).

## Запуск и службы

- `oneshot` запускается командой `byam run`.
- Включённый `service` автоматически запускается после перезапуска BetterYAMd.
- Lua-службы должны регулярно уступать выполнение через `alice.sleep()` или
  другой подходящий вызов `alice.*`, чтобы остановку можно было обработать.

`byam service` управляет пакетами с `launch.mode = "service"`:

| Команда | Действие |
| --- | --- |
| `list` | Показать службы и их состояние |
| `enable <id>` | Запустить службу и включить автозапуск |
| `disable <id>` | Остановить службу и отключить автозапуск |
| `start <id>` | Запустить включённую службу |
| `stop <id>` | Остановить службу, не меняя автозапуск |
| `restart <id>` | Перезапустить включённую службу |
| `logs <id>` | Показать журнал службы |

Прежние команды `byam enable`, `disable`, `stop` и `logs` сохранены. Команда
`yam` также доступна как совместимый alias `byam`.

## Binary-пакеты

Binary-пакету требуется permission `exec`; исполняемый файл должен подходить
архитектуре устройства. В манифесте можно задать аргументы, `interpreter = "sh"`
и `library_paths`.

После установки BetterYAM создаёт wrapper в
`/data/quazar/betteryam/data/bin` и, если системная файловая система доступна
для записи, ссылку в `/usr/bin`. Это позволяет вызывать пакет по его ID в
`adb shell`, даже если login profile не загружен. Для login shell каталог
wrapper-ов также добавляется в `/etc/profile.d/betteryam.sh`.

Если `/usr/bin/<package-id>` уже занят, существующая команда сохраняется, а
конфликт записывается в журнал. При read-only системной файловой системе пакет
устанавливается, но wrapper станет доступен из `adb shell` только после того,
как daemon сможет создать ссылку. При обновлении или удалении пакета
управляемая команда также обновляется или удаляется.

Команда запускается из каталога пакета с `launch.args` и указанными
библиотеками. Ей не передаются `YAM_API_URL` и `YAM_API_TOKEN`. Binary-пакеты
работают с правами пользователя shell, поэтому устанавливайте только
доверенные пакеты. Вывод запуска через runtime доступен в `byam logs <id>`.

## Команды `byam`

```sh
byam list
byam list --json
byam registry search audio
byam install package.yap
byam install nano
byam install nano 8.0
byam service list
byam service enable ambient-lights
byam run nano -- --help
byam tui nano
byam rm nano
```

`byam install` принимает путь к локальному `.yap`/`.pkg`-архиву или ID пакета в
registry. Если локальный архив с таким расширением не найден, команда завершится
ошибкой, а не станет трактовать путь как ID. Для пакета из registry можно
указать версию; без неё устанавливается последняя версия.

`byam registry pull` — alias `byam registry install`. Команды registry
поддерживают `--registry URL`, а запросы к daemon — `--url URL`; параметры можно
указывать до или после подкоманды. Также поддерживаются `YAM_REGISTRY_URL` и
`YAM_URL`.

Версию CLI можно посмотреть через `byam -v` или `byam --version`. Цветной вывод
автоматически включается в интерактивном терминале. Настройте его глобальной
опцией `--color auto|always|never` или переменной `BYAM_COLOR`; `NO_COLOR`
отключает цвета в режиме `auto`. JSON-вывод не содержит ANSI-кодов.

## Runtime-контракты

Binary runtime получает `YAM_API_URL` и одноразовый `YAM_API_TOKEN`. Передавайте
токен в заголовке `X-YAM-Token`. Доступность runtime-маршрутов зависит от
permissions пакета; общий API описан в
[документации развёртывания](./DEPLOYMENT.md).

Raw Quasar IPC предоставляется только Lua bindings и не добавляет IPC-маршруты
в binary runtime API. Для Lua задайте `ipc.read` и/или `ipc.control`, а также
allowlist в `[ipc].services`. Подробнее — в разделе
[Input и IPC](./PLUGIN_API.md#ipc).
