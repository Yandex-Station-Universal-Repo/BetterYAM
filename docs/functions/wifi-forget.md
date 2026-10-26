# `alice.wifi.forget(ssid)`

Удаляет сохранённую Wi-Fi сеть.

- **Permission:** `wifi`
- **Аргумент:** SSID.
- **Результат:** boolean/result операции; ошибка передаётся Lua.
- **Внимание:** удаление текущей сети может разорвать соединение.

```lua
alice.wifi.forget("Guest Wi-Fi")
```