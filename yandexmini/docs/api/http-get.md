# `alice.http.get(url)`

Выполняет HTTP GET к разрешённому manifest host.

- **Permission:** `network`
- **Аргумент:** URL с hostname из `[network].allow`.
- **Результат:** таблица `{status, body}`.
- **Ограничения:** HTTPS обязателен, если явно не разрешён
  `allow_insecure_http`; loopback/private IP и URL с credentials запрещены.

```lua
local response = alice.http.get("https://api.example.org/status")
```

Manifest требует непустой список разрешённых hostnames.
