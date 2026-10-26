# `alice.bluetooth.pair(address)`

Запрашивает сопряжение с Bluetooth-устройством.

- **Permission:** `bluetooth`
- **Аргумент:** MAC-адрес; формат проверяется до вызова backend.
- **Доступность Mini:** нет (`bluetooth.pair=false`); операция не реализована.
- **Результат:** на поддерживающем backend — `nil` либо ошибка API.

```lua
alice.bluetooth.pair("01:23:45:67:89:ab")
```

Входящее pairing было проверено вручную через `bt_demo` после включения
discoverable и connectable режимов. Это не проверяет и не включает
`alice.bluetooth.pair`; BetterYAM pairing API на Mini не поддерживает.
Созданная для проверки пара удалена.
