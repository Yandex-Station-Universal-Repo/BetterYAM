# BetterYAM

**BetterYAM — альтернативный runtime для YAM-плагинов на Яндекс Станциях.**
Он запускает Lua- и binary-пакеты, предоставляет CLI `byam` и HTTP API для
управления устройством.

Общий HTTP API и plugin runtime находятся в `betteryam-runtime/`. Каждый
поддерживаемый тип устройства подключает собственный adapter, который
предоставляет аппаратные backend-ы и профиль возможностей. Доступность API
определяется профилем, а не предположением о модели.

> Сейчас в репозитории есть daemon-ы для Yandex Station Mini и Lite
> (`yandexmicro`). Неизвестная модель не подменяется похожей: для новой модели
> нужны отдельные реализация и проверка.

## Возможности

- Запуск Lua- и binary-пакетов `.yap`/`.pkg` с сохранением совместимости YAM.
- CLI `byam` для установки и управления пакетами, службами и журналами.
- HTTP API для CLI, Web Panel и binary-плагинов.
- Device profiles и capabilities для проверки доступности аппаратных функций.
- Package permissions для контроля доступа к аппаратным и системным API.
- Установка daemon-а на подключённую по ADB колонку с проверкой SHA-256.

## Установка

Требуются `adb` и поддерживаемая Станция, доступная по ADB и сети.

### macOS и Linux

Установщик загружает CLI на компьютер и daemon для подключённой Станции:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.sh | sh
```

Для установки конкретной версии передайте номер релиза:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.sh | sh -s -- 0.1.18
```

CLI устанавливается в `~/.local/bin`. Основная команда — `byam`; `yam` оставлен
как совместимый alias. Если команда не найдена, добавьте этот каталог в
`PATH`.

### Windows

Запустите PowerShell от имени пользователя, у которого установлен `adb`:

```powershell
irm https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.ps1 | iex
```

### Обновление и удаление

Установщик выбирает daemon по hostname и прекращает работу, если точной модели
нет в release manifest. Для удаления из клона репозитория выполните:

```sh
sh install.sh --uninstall
```

Подробности установки, перемонтирования файловой системы и startup hook
приведены в [руководстве по развёртыванию](./docs/DEPLOYMENT.md).

## Первые команды

```sh
byam device
byam list
byam registry search lights
byam install package.yap
byam install nano
byam run nano
byam logs nano
byam --help
```

Большинство команд имеют `--help`. Для скриптов используйте JSON, например
`byam list --json`. Службами можно управлять через `byam service`; полный
список команд и примеры приведены в [руководстве по пакетам](./docs/PACKAGES.md).

## API и совместимость

Плагины сохраняют привычные Lua-глобальные значения `alice`, `device` и
`event`, основные вызовы `alice.set`, `alice.off`, `alice.play`,
`alice.logo.set` и `alice.logo.off`, а также permissions существующего
контракта.

Ответ `GET /api/device` содержит совместимые `features` и boolean-словарь
`capabilities`. Проверяйте capability конкретной операции: например,
`bluetooth.status` не гарантирует доступность `bluetooth.power`. Для LED
используйте `device.led_count`; не зашивайте число элементов в плагин.

Binary runtime сохраняет `YAM_API_URL`, `YAM_API_TOKEN` и заголовок
`X-YAM-Token`. Доступность аппаратных API определяется профилем адаптера.
Подробности:

- [Общий Lua API и permissions](./docs/PLUGIN_API.md)
- [Функции Lua API](./docs/functions/README.md)
- [Формат пакетов и CLI](./docs/PACKAGES.md)
- [HTTP API и развёртывание](./docs/DEPLOYMENT.md)

## Поддержка устройств

Mini и Lite используют разные LED backend-ы и профили. Другие аппаратные
возможности также могут различаться в зависимости от модели, прошивки,
обнаруженного оборудования и системных интерфейсов. Смотрите
[матрицу поддержки](./docs/DEVICE_SUPPORT.md).

Для добавления устройства требуется отдельный adapter и daemon, запись в
корневом [`manifest.json`](./manifest.json) и проверка совместимости. Следуйте
[руководству по добавлению устройства](./docs/ADDING_DEVICES.md); общий runtime
и Lua API не должны содержать модельно-специфичных ветвлений.

## Сборка и тестирование

Для локальной сборки и установки daemon-а используйте
[скрипт сборки и деплоя](./tools/build-deploy.sh):

```sh
./tools/build-deploy.sh
```

Скрипт предложит выбрать daemon и подключённую колонку. Устройство и daemon
можно указать явно:

```sh
./tools/build-deploy.sh yandexmini 192.168.0.171:5555
```

Для сборки в Docker добавьте `--docker`. Перед деплоем скрипт сверяет модель
устройства с aliases daemon-а и при ошибке запускает попытку отката.

Основные проверки:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
sh -n install.sh
sh -n tools/build-daemons.sh
```

Релиз публикуется GitHub Actions при push тега `v*`. Версия тега должна
соответствовать версии CLI в корневом `Cargo.toml`.

## Документация

| Тема | Документ |
| --- | --- |
| Установка, HTTP API и релизы | [DEPLOYMENT.md](./docs/DEPLOYMENT.md) |
| Lua API и permissions | [PLUGIN_API.md](./docs/PLUGIN_API.md) |
| Пакеты и команды CLI | [PACKAGES.md](./docs/PACKAGES.md) |
| Добавление устройства | [ADDING_DEVICES.md](./docs/ADDING_DEVICES.md) |
| Интеграция Web Panel | [WEB_PANEL_INTEGRATION.md](./docs/WEB_PANEL_INTEGRATION.md) |
| Профили и ограничения устройств | [DEVICE_SUPPORT.md](./docs/DEVICE_SUPPORT.md) |
| Lua-функции | [docs/functions/](./docs/functions/README.md) |

## Участие в разработке

Перед изменением проверьте действующий API-контракт. Для нового устройства
укажите проверенные hostname aliases, архитектуру, capabilities и результаты
тестирования. Не включайте в репозиторий credentials, пароли, пользовательские
данные или неочищенные логи.

BetterYAM — самостоятельный проект и не является официальным продуктом
Яндекса. Лицензия: [MIT](./LICENSE).
