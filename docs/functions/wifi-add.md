# `alice.wifi.add(ssid, password?)`

Добавляет Wi-Fi сеть в конфигурацию `wpa_supplicant`.

- **Permission:** `wifi`
- **Аргументы:** SSID и необязательный пароль; пустой/отсутствующий пароль
  означает открытую сеть.
- **Результат:** boolean/result операции; ошибки настройки передаются Lua.
- **Внимание:** пакет может менять сеть устройства и временно терять связь.

```lua
alice.wifi.add("Home Wi-Fi", "correct horse battery")
```