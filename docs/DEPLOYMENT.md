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

## Безопасность и обновления

Web UI не имеет аутентификации: используйте только доверенную LAN. Автообновление включено; отключение в `settings.toml`:

```toml
[updates]
enabled = false
```
