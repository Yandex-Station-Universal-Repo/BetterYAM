# Пакеты

`.yap` — ZIP или `tar.gz` с `plugin.toml` в корне. Пути внутри пакета только относительные.

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

`oneshot` запускается через `byam run`; включённый `service` восстанавливается после перезапуска BetterYAMd. Lua-сервисы должны регулярно вызывать `alice.sleep` или `alice.set`.

Binary-пакет требует `exec`, должен соответствовать архитектуре колонки (обычно AArch64 musl) и может задать `args`, `interpreter = "sh"` и `library_paths`. Его вывод доступен через `byam logs <id>`.

CLI показывает версию через `byam -v` или `byam --version`. Цветной вывод
включается автоматически в интерактивном терминале. Режим можно переопределить
глобальной опцией `--color auto|always|never` или переменной `BYAM_COLOR`;
`NO_COLOR` отключает цвета в режиме `auto`. JSON-команды `--json` остаются без
ANSI-кодов, поэтому их вывод можно напрямую передавать другим инструментам.

```sh
byam list                         # установленные пакеты и их состояние
byam list --json                  # тот же список в JSON для скриптов
byam registry list                # каталог registry
byam registry search audio        # поиск по ID, имени и описанию
byam registry list audio          # тот же поиск через list
byam install package.yap          # локальный архив
byam install nano                 # последняя версия из registry
byam install nano 8.0             # конкретная версия registry
byam registry install nano 8.0    # то же через registry subcommand
byam registry list --json         # каталог в JSON
byam enable nano
byam service list
byam service enable ambient-lights
byam service restart ambient-lights
byam service logs ambient-lights
byam run nano -- --help
byam tui nano                     # интерактивный binary oneshot через ADB
byam stop nano
byam rm nano
```

`byam install` принимает путь к существующему `.yap`/`.pkg` архиву или ID
пакета из registry. Если локальный архив с таким расширением не найден, CLI
сообщит об ошибке вместо попытки трактовать путь как registry ID. Установка
показывает имя и версию добавленного пакета; `--json` сохраняет машинный ответ
daemon. `byam registry pull` оставлен как alias для `byam registry install`.
Параметры `--registry URL` и `--url URL` можно указывать до или после
подкоманды; также поддерживаются `YAM_REGISTRY_URL` и `YAM_URL`.

`byam service` управляет только пакетами с `launch.mode = "service"`:
`enable` запускает сервис и сохраняет автозапуск, `disable` останавливает его
и отключает автозапуск, `start` запускает включённый сервис, `stop` останавливает
без отключения, `restart` перезапускает включённый сервис, `list` показывает
установленные сервисы и их состояние, а `logs` выводит журнал сервиса.
Прежние команды `byam enable`, `byam disable`, `byam stop` и `byam logs`
сохранены для обратной совместимости. В установках также доступна команда
`yam` как alias для `byam`.

Binary runtime получает `YAM_API_URL` и одноразовый `YAM_API_TOKEN`; передавайте токен в `X-YAM-Token`. Доступны `/device`, `/sensors`, `/led`, `/led/off`, `/logo`, `/logo/off` и `/clock` согласно permissions пакета. Hardware операции маршрутизируются через Device API текущей модели BetterYAMd.

Raw Quasar IPC (`alice.ipc.next`, `send`, `request`) доступен Lua-пакетам с
permissions `ipc.read` и/или `ipc.control`. Перечислите разрешённые service
names в `[ipc].services`; см. [Lua API](./PLUGIN_API.md#input-и-ipc) для
ограничений и примера. Эти Lua bindings не добавляют IPC endpoints в binary
runtime API.
