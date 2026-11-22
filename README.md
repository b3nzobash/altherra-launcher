<p align="center"><img src="assets/icon.png" width="112" alt=""></p>

<h1 align="center">Altherra Launcher</h1>

<p align="center">Лаунчер Minecraft-сервера <b>Altherra</b>. Швидкий, нативний, без зайвого.</p>

---

- **Ліцензія й без неї.** Вхід через Microsoft або просто за ніком; перемикання між акаунтами в один клік.
- **Ставить усе сам.** Java, Minecraft, Fabric і моди сервера: нічого не треба шукати й копіювати руками.
- **Завжди актуальна збірка.** Коли на сервері оновлюються моди, лаунчер підтягне зміни перед наступним запуском.
- **Легкий.** Один файл на Rust + [Slint](https://slint.dev) без браузера всередині: відкривається миттєво й майже не їсть памʼяті.
- **Надійний.** Кожен файл перевіряється за контрольною сумою, а невдалі завантаження повторюються. Лаунчер ходить в інтернет лише через HTTPS і не дає серверу збірки виконати щось поза грою.

## Дві версії

| | **Altherra** | **Altherra Offline** |
|---|---|---|
| Вхід з ліцензією Microsoft | ✅ | — |
| Гра за ніком | після входу з ліцензійним акаунтом | ✅ одразу |
| Код входу Microsoft і Client ID | є | немає зовсім |

Обидві ставлять Java, Minecraft, Fabric і моди сервера, оновлюються самі й можуть стояти поруч.

## Завантажити

Останній реліз — на сторінці [Releases](../../releases/latest):

| Система | Altherra | Altherra Offline |
|---|---|---|
| Windows 10/11 | `Altherra-Setup-windows-x64.exe` | `Altherra-Offline-Setup-windows-x64.exe` |
| Linux (glibc і musl) | `Altherra-Setup-linux-x64.run` | `Altherra-Offline-Setup-linux-x64.run` |
| macOS 11+ (Apple Silicon та Intel) | `Altherra-macos.zip` | `Altherra-Offline-macos.zip` |

Інсталятор для Linux запускається так: `sh Altherra-Setup-linux-x64.run`.

Встановлений лаунчер оновлюється сам: коли виходить нова версія, зверху зʼявляється кнопка «Оновити». Оновлення підписані ключем Ed25519, тож підмінений файл лаунчер не встановить.

## Збирання

```bash
cargo build --release                        # Altherra
cargo build --release --no-default-features  # Altherra Offline
```

---

Не є офіційним продуктом Minecraft. Не схвалено Mojang чи Microsoft і не повʼязано з ними. Шрифти Unbounded, Manrope і JetBrains Mono — за ліцензією SIL OFL 1.1 (`ui/fonts`). Made with Slint.
