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

`oneshot` запускается через `yam run`; включённый `service` восстанавливается после перезапуска BetterYAMd. Lua-сервисы должны регулярно вызывать `alice.sleep` или `alice.set`.

Binary-пакет требует `exec`, должен соответствовать архитектуре колонки (обычно AArch64 musl) и может задать `args`, `interpreter = "sh"` и `library_paths`. Его вывод доступен через `yam logs <id>`.

```sh
yam install package.yap          # локальный архив
yam install nano                 # последняя версия из registry
yam install nano 8.0             # конкретная версия
yam enable nano
yam run nano -- --help
yam tui nano                     # интерактивный binary oneshot через ADB
yam stop nano
yam rm nano
```

Binary runtime получает `YAM_API_URL` и одноразовый `YAM_API_TOKEN`; передавайте токен в `X-YAM-Token`. Доступны `/device`, `/sensors`, `/led`, `/led/off`, `/logo`, `/logo/off` и `/clock` согласно permissions пакета. Hardware операции маршрутизируются через Device API текущей модели BetterYAMd.
