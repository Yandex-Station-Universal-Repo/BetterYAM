# `alice.input.devices()`

Возвращает доступные input device names, ограниченные manifest-политикой
пакета.

- **Permission:** `input`
- **Аргументы:** нет.
- **Результат:** массив строк с точными kernel device names.
- **Доступность Mini:** да для устройств, доступных daemon-процессу.

```lua
for _, name in ipairs(alice.input.devices()) do
    print(name)
end
```

В `plugin.toml` явно перечислите разрешённые имена, например:

```toml
permissions = ["input"]

[input]
devices = ["adc_keypad"]
```

Указанные в manifest имена ограничивают список; произвольные `/dev/input`
плагину не выдаются.
