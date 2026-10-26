# `alice.http.post(url, body)`

Выполняет HTTP POST к разрешённому manifest host.

- **Permission:** `network`
- **Аргументы:** разрешённый URL и строка тела не более 1 MiB.
- **Результат:** таблица `{status, body}`.
- **Ограничения:** те же host/scheme ограничения, что у
  [`alice.http.get`](http-get.md).

```lua
local response = alice.http.post("https://api.example.org/events", "{\"ok\":true}")
```