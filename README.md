# okpo

Ежедневная и ручная выгрузка реестров вагонов с корпоративной UNC-шары (Windows) на Ubuntu по SFTP, затем запуск `okpo-agent` на prod с reverse SOCKS (`ssh -R 3128`).

## Назначение

1. Находит пакет из **четырёх** самых свежих реестров в каталоге `Реестры` на файловом сервере.
2. Копирует их во временную папку `tmp/` в корне проекта **с исходными именами**.
3. Передаёт пакет на Ubuntu по SSH/SFTP в `remote_dir` **с теми же именами** (один SFTP-сеанс, библиотека russh).
4. Удаляет локальные копии из `tmp/` после успешной загрузки.
5. Открывает вторую сессию **OpenSSH CLI**: `ssh -R 3128 …` и в ней запускает `okpo-agent` (DaData/Yandex через SOCKS на prod `127.0.0.1:3128`, трафик через эту Windows-машину). Сессия живёт, пока идёт register, затем закрывается.

По умолчанию агент работает как демон и каждый день в **04:00 Europe/Moscow** выполняет автопоиск, загрузку и запуск register (см. `config.toml`).

## Требования

- Windows-машина с доступом к UNC-шаре и **выходом в интернет** (через неё идёт SOCKS для DaData)
- Rust toolchain (`cargo`, `rustc`)
- OpenSSH-клиент в PATH (`ssh` / `ssh.exe`) — для шага `-R` + remote command
- SSH private key с доступом к Ubuntu-хосту
- На Ubuntu включён SFTP; для `-R` в `sshd_config` обычно нужно `GatewayPorts`/`AllowTcpForwarding` (по умолчанию localhost-bind ок)
- На Ubuntu в env okpo-agent: `OKPO_HTTP_PROXY=socks5h://127.0.0.1:3128` (или аналог)

## Конфигурация

Файл [`config.toml`](config.toml) ищется сначала в текущей директории, затем в корне проекта (`CARGO_MANIFEST_DIR`). Если файл не найден — используются значения по умолчанию из кода.

```toml
[source]
base_unc = "\\\\mskfs.rusagrotrans.ru\\Groups\\...\\Реестры"

[schedule]
timezone = "Europe/Moscow"
hour = 4
minute = 0

[ssh]
host = "10.101.139.4"
port = 22
user = "atretyakov"
private_key = "C:\\Users\\tretyakov_av\\.ssh\\id_rsa"
remote_dir = "/home/atretyakov/okpo-agent/data/registers"

[agent]
enabled = true
remote_socks_port = 3128
working_directory = "/home/atretyakov/okpo-agent"
remote_command = "OKPO_SKIP_BUILD=1 ./run.sh prod register --dadata-parse"
ssh_binary = "ssh"
```

| Секция | Поле | Описание |
|--------|------|----------|
| `source` | `base_unc` | Корневая папка реестров на шаре |
| `schedule` | `timezone` | Таймзона расписания (IANA) |
| `schedule` | `hour` / `minute` | Время ежедневного запуска |
| `ssh` | `host` / `port` / `user` | Параметры SSH |
| `ssh` | `private_key` | Путь к приватному ключу |
| `ssh` | `remote_dir` | Каталог на Ubuntu для реестров |
| `agent` | `enabled` | После SFTP запускать okpo-agent с `-R` |
| `agent` | `remote_socks_port` | Порт remote dynamic SOCKS (`ssh -R <port>`) |
| `agent` | `working_directory` | `cd` на Ubuntu перед командой |
| `agent` | `remote_command` | Команда register на Ubuntu |
| `agent` | `ssh_binary` | OpenSSH-клиент (`ssh` / `ssh.exe`) |

## Логика выбора файлов (автопоиск пакета)

Время считается в таймзоне из `config.toml` (по умолчанию Москва).

1. Рассматриваются реестры с датой **строго меньше сегодняшней**.
2. Просматриваются **две** папки месяцев:
   - месяц даты «вчера» (год `YYYY`, папка `{РусскоеИмя} {YY}`, например `Август 26`);
   - **предыдущий** месяц (в начале месяца / года — соседняя папка, в т.ч. `Декабрь` прошлого года).
3. Среди файлов вида `Реестр DD.MM..xls` / `Реестр DD.MM.xls` выбираются **4 самых поздних** по дате.
4. Если найдено меньше 4 файлов — ошибка (прогон завершается с ненулевым кодом).

Имена на Ubuntu совпадают с исходными именами файлов. Режим `--file` по-прежнему загружает **один** указанный файл.

## Поведение при существующем файле на Ubuntu

Перед записью проверяется наличие remote-файла. Если он уже есть — в лог пишется предупреждение, файл **перезаписывается**. Это не считается ошибкой.

## Команды

```bat
:: проверка и сборка
cargo check
cargo build
cargo build --release
cargo test

:: справка по флагам
cargo run -- --help

:: демон: ежедневно по schedule из config.toml (SFTP + agent)
cargo run --release
.\target\release\okpo.exe

:: разовый автопоиск пакета (4 файла), SFTP, затем ssh -R + register
cargo run -- --once
cargo run --release -- --once
.\target\release\okpo.exe --once

:: только выгрузка файлов, без запуска okpo-agent
.\target\release\okpo.exe --once --skip-agent

:: ручная загрузка одного файла (+ agent, если enabled)
cargo run -- --file "Реестр 22.07..xls"
cargo run --release -- --file "Реестр 22.07..xls"
.\target\release\okpo.exe --file "Реестр 22.07..xls"
```

Уровень логов можно задать через `RUST_LOG` (по умолчанию `info`):

```bat
set RUST_LOG=debug
cargo run -- --once
```

Логи каждого запуска пишутся в `okpo-task.log` в корне проекта (рядом с `config.toml`).
Файл **перезаписывается** при каждом прогоне — хранится только последний запуск.

## Режимы CLI

| Режим | Команда | Поведение |
|-------|---------|-----------|
| Демон | `okpo` | Ждёт расписания, каждый день пакет + SFTP + agent |
| Разовый авто | `okpo --once` | Один прогон: пакет из 4 реестров + SFTP + agent |
| Ручная загрузка | `okpo --file "…"` | Один файл + SFTP + agent |
| Без agent | `… --skip-agent` | Только SFTP (удобно для отладки выгрузки) |

Флаг `--file` одноразовый: после прогона процесс завершается. Не комбинируется с `--once`.

### Почему два SSH-канала

| Шаг | Как | Зачем |
|-----|-----|-------|
| SFTP | russh в процессе `okpo` | Надёжная выгрузка файлов |
| `-R` + register | системный `ssh` | Remote dynamic SOCKS (`-R 3128`) как у ручного `ssh -R 3128`; russh так не умеет |

Пока идёт `remote_command`, туннель жив; после exit register сессия закрывается — systemd-таймер на prod без живого `-R` для DaData не подходит.

## Эксплуатация

- Для срабатывания встроенного таймера процесс должен быть **запущен и не завершён** (сессия пользователя / служба / автозагрузка).
- Альтернатива: Windows Task Scheduler с ежедневным запуском `okpo.exe --once` (см. [WINDOWS_TASK_SCHEDULER.md](WINDOWS_TASK_SCHEDULER.md)). Учтите: register может идти долго — увеличьте лимит времени задачи.
- ПК должен быть **онлайн** на время register (SOCKS через Windows).
- Временные файлы лежат в `tmp/` в корне проекта и удаляются после **успешной** загрузки на Ubuntu.
