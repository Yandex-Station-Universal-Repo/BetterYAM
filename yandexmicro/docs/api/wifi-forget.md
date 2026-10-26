# `alice.wifi.forget(ssid)`

Удаляет сохранённую Wi-Fi сеть.

- **Permission:** `wifi`
- **Аргумент:** SSID.
- **Результат:** boolean/result операции; ошибка передаётся Lua.
- **Доступность Mini:** да при доступном Wi-Fi adapter.
- **Внимание:** удаление текущей сети может разорвать соединение.

```lua
alice.wifi.forget("Guest Wi-Fi")
```
