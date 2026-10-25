# Добавление поддержки нового устройства

Это руководство помогает добавить в BetterYAM новую модель колонки или
устройства. Его можно выполнять вручную или передать coding-агенту вместе с
репозиторием.

## Как устроена поддержка устройств

Сейчас каждое семейство устройств реализуется отдельным daemon crate в
собственном каталоге. Daemon предоставляет HTTP API и обслуживает hardware
этой модели. CLI `byam` подключается к daemon, а установщик выбирает daemon по
hostname устройства и корневому [`manifest.json`](../manifest.json).

Для примера структуры и текущего API изучите [`yandexmini/`](../yandexmini/),
но не копируйте Mini hardware код и его ограничения в новую модель. Число
светодиодов, наличие функций и другие аппаратные свойства должны определяться
реальным устройством.

## Что выяснить до реализации

Запишите в задаче или PR:

- точную модель и все варианты hostname из `uname -n`;
- архитектуру и ABI устройства, версию ОС и доступную среду сборки;
- способ подключения, установки и запуска daemon, необходимые права;
- hardware интерфейсы, kernel devices, системные утилиты и vendor API;
- какие функции реально можно обнаружить и проверить на устройстве;
- поведение при отсутствии устройства, зависимости, разрешения или daemon;
- какие функции и HTTP-маршруты нужны для существующих плагинов.

Не добавляйте в репозиторий credentials, токены, Wi-Fi пароли, пользовательские
данные, дампы устройства или логи с секретами. Для исследования сохраняйте
только минимальные обезличенные сведения, необходимые для реализации.

Текущие Unix и Windows установщики находят колонку через `adb`, проверяют
поддерживаемую ARM-архитектуру и выбирают daemon по hostname. Если новая модель
не доступна по ADB, использует другую архитектуру или требует другого способа
установки, одной записи в manifest недостаточно: отдельно спроектируйте и
проверьте изменения обоих установщиков. Не обходите это ограничение,
направляя модель на несовместимый asset.

## Порядок изменений

1. **Проверьте текущий контракт.** Прочитайте [`AGENTS.md`](../AGENTS.md),
   [`docs/PLUGIN_API.md`](./PLUGIN_API.md),
   [`docs/PACKAGES.md`](./PACKAGES.md) и
   [`docs/DEPLOYMENT.md`](./DEPLOYMENT.md). Сверьте маршруты и поля профиля с
   текущим daemon в `yandexmini/main.rs` и аппаратными модулями в `yandexmini/`.
   Не меняйте API плагинов или контракт `byam`, если задача явно не требует
   отдельного совместимого изменения.

2. **Добавьте отдельный daemon crate.** Создайте каталог с коротким
   lowercase ID, например `vendor_model/`, со своим `Cargo.toml`, исполняемым
   daemon и hardware-модулями. Добавьте crate в явный список `members`
   корневого `Cargo.toml`, чтобы он участвовал в workspace checks и tests.
   Держите model-specific HTTP API,
   обнаружение hardware и драйверы внутри каталога устройства.

3. **Сохраните HTTP-контракт BetterYAM.** Совместимый daemon должен
   предоставлять API и runtime endpoints, которые ожидают CLI и существующие
   плагины. Сверьте реализацию и permissions с документацией и маршрутами
   текущего daemon. Не удаляйте и не меняйте поведение существующих маршрутов
   ради особенностей новой модели. Поддерживаемую, но отсутствующую функцию
   возвращайте как unavailable с понятной ошибкой, а не как успешную заглушку.

4. **Опишите реальные возможности устройства.** В профиле `GET /device`
   укажите `id`, `model`, `architecture`, `led_count`, `features` и boolean
   `capabilities`. Устанавливайте capability в `true`, только когда адаптер
   обнаружен и операция доступна. Сохраняйте именование существующих
   capability, например `led.ring.rgb` и `led.ring.<count>`. Не объявляйте
   возможности на основании одной только модели, если их доступность зависит
   от hardware, driver или runtime-состояния.

5. **Зарегистрируйте сборку и модели.** Добавьте запись в корневой
   [`manifest.json`](../manifest.json), задав уникальные `device`, Cargo
   `manifest_path`, имя `binary`, имя release `asset`, Rust `target` и список
   `models`. Модели должны соответствовать нормализованному hostname:
   установщик удаляет конечный сегмент `-XXXX`/`_XXXX`, состоящий из четырёх
   заглавных букв или цифр, затем приводит строку к lowercase и заменяет
   неподдерживаемые разделители дефисами. Добавляйте только точные проверенные
   aliases. Не направляйте неизвестную модель на daemon похожего устройства.

   Пример записи (замените значения на реальные):

   ```json
   {
     "device": "vendor_model",
     "models": ["vendor-model", "vendor-model-revision"],
     "manifest_path": "vendor_model/Cargo.toml",
     "binary": "betteryamd-vendor-model",
     "asset": "betteryamd-vendor-model-aarch64-linux",
     "target": "aarch64-unknown-linux-musl",
     "filepath": "dist/assets/betteryamd-vendor-model-aarch64-linux",
     "sha256sum": null
   }
   ```

   `filepath` и `sha256sum` в исходном корневом manifest служат описанием
   будущего release asset; `tools/build-daemons.sh` создаёт release manifest
   и подставляет фактический путь и SHA-256 при сборке. Формат записи должен
   соответствовать уже существующим записям.

6. **Проверьте установку и release-сборку.** Запустите `byam` и installer
   против нового daemon на устройстве, если hardware доступен. Проверьте
   правильный выбор модели и отсутствие установки чужого daemon. Скрипт
   `tools/build-daemons.sh` собирает все записи manifest и генерирует
   `dist/manifest.json` и `dist/SHA256SUMS`; не добавляйте `dist/`, бинарники
   или package archives в git.

## Проверки перед PR

Запускайте проверки, подходящие для изменений:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets --target x86_64-unknown-linux-musl
cargo test --workspace --target x86_64-unknown-linux-musl
sh -n tools/build-daemons.sh
sh -n install.sh
```

Проверьте target-specific release build. При наличии Docker используйте:

```sh
BUILD_WITH_DOCKER=1 sh tools/build-daemons.sh
```

Этот скрипт собирает все daemon entries из manifest, а не только новый crate.
Если локальная среда не поддерживает целевой Rust target, явно укажите это в
PR и приложите результат подходящей CI-сборки. Не заявляйте успешную работу
hardware, если её не проверяли на целевом устройстве.

В описании PR перечислите:

- модели и проверенные hostname aliases;
- архитектуру, target и команду сборки;
- добавленные HTTP endpoints и capabilities;
- фактические hardware-проверки и непроверенные функции;
- необходимые permissions, системные утилиты и ограничения устройства.

## Запрос для coding-агента

Скопируйте этот текст в coding-агенте, который запущен в корне репозитория.
Заполните сведения об устройстве. Не передавайте секреты или пользовательские
данные.

```text
Добавь поддержку нового устройства в BetterYAM в этом репозитории.

Устройство:
- Производитель и модель: <указать>
- Примеры `uname -n`: <указать реальные hostname>
- Архитектура / ABI / ОС: <указать или отметить неизвестное>
- Доступ к устройству для тестирования: <есть / нет, какие интерфейсы>
- Требуемые функции: <перечислить>
- Известные ограничения или hardware interfaces: <перечислить>

Работай до готового изменения, а не только до плана:
1. Прочитай AGENTS.md, docs/ADDING_DEVICES.md, docs/PLUGIN_API.md,
   docs/PACKAGES.md, docs/DEPLOYMENT.md, root manifest.json,
   tools/build-daemons.sh и текущий daemon в yandexmini/.
2. Сначала определи текущий HTTP/API и plugin-runtime contract, workspace,
   build target и release manifest format. Не предполагай, что Mini-specific
   драйвер или его аппаратные параметры подходят новому устройству.
3. Реализуй отдельный daemon crate в каталоге с lowercase ID. Изолируй
   hardware implementation и model-specific детали в каталоге устройства.
   Поддержи существующий API и совместимость плагинов; не меняй Lua API,
   permission model или CLI для обхода отсутствующей hardware-функции.
4. Добавь в Device profile только реально поддерживаемые features и boolean
   capabilities. Не объявляй неподтверждённые функции доступными; возвращай
   понятную ошибку для недоступных операций.
5. Добавь crate в `members` корневого Cargo.toml и зарегистрируй daemon в
   root manifest.json с корректными Cargo path, binary, target, asset и
   точными нормализованными hostname aliases. Не добавляй сопоставление по
   похожему имени и не направляй новые модели на другой daemon.
6. Добавь/обнови тесты для discovery/profile, capabilities, input validation
   и API behavior в соответствии с изменениями. Не включай credentials,
   реальные пароли, логи с секретами или device dumps.
7. Обнови относящуюся к устройству документацию и инструкцию установки,
   если изменились реальные требования. Не редактируй несвязанные файлы.
8. Запусти cargo fmt --all --check, cargo check --workspace --all-targets
   --target x86_64-unknown-linux-musl, cargo test --workspace
   --target x86_64-unknown-linux-musl, sh -n tools/build-daemons.sh и
   sh -n install.sh. Проверь release-сборку target; при возможности запусти
   BUILD_WITH_DOCKER=1 sh tools/build-daemons.sh.
9. В конце перечисли изменённые файлы, точные команды и результаты проверок.
   Явно отдели функции, проверенные на устройстве, от тех, которые не удалось
   проверить. Не утверждай, что build/test/device validation прошли, если
   соответствующая команда или проверка не выполнялась.
```
