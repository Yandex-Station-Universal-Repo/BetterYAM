# `alice.volume.set(percent)`

Устанавливает уровень громкости.

- **Permission:** `volume`
- **Аргумент:** целое число `0..100`.
- **Результат:** `nil` при успехе; ошибка ALSA передаётся Lua.
- **Доступность Mini:** при обнаружении ALSA control `Master` на card 0.

```lua
alice.volume.set(35)
```
