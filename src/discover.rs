use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Datelike, NaiveDate};
use regex::Regex;
use std::sync::OnceLock;

const MONTH_NAMES_RU: [&str; 12] = [
    "Январь",
    "Февраль",
    "Март",
    "Апрель",
    "Май",
    "Июнь",
    "Июль",
    "Август",
    "Сентябрь",
    "Октябрь",
    "Ноябрь",
    "Декабрь",
];

fn register_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)Реестр\s+(\d{1,2})\.(\d{1,2})\.+xls$")
            .expect("valid register filename regex")
    })
}

/// Папка месяца: `Июль 26`.
pub fn month_folder_name(date: NaiveDate) -> String {
    let month_name = MONTH_NAMES_RU[(date.month0()) as usize];
    let yy = date.format("%y");
    format!("{month_name} {yy}")
}

pub fn year_folder_name(date: NaiveDate) -> String {
    date.year().to_string()
}

/// Дата реестра для автопоиска: вчера по календарю «сегодня» (Москва передаётся снаружи).
pub fn target_register_date(today: NaiveDate) -> NaiveDate {
    today
        .pred_opt()
        .expect("NaiveDate::pred_opt should succeed for civil dates")
}

/// Парсит DD.MM из имени вида `Реестр 22.07..xls` / `Реестр 22.07.xls`.
pub fn parse_register_filename(filename: &str) -> Option<(u32, u32)> {
    let caps = register_date_re().captures(filename)?;
    let day: u32 = caps.get(1)?.as_str().parse().ok()?;
    let month: u32 = caps.get(2)?.as_str().parse().ok()?;
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return None;
    }
    Some((day, month))
}

/// Год для даты DD.MM относительно «сегодня».
pub fn year_for_day_month(day: u32, month: u32, today: NaiveDate) -> i32 {
    let y = today.year();
    if let Some(candidate) = NaiveDate::from_ymd_opt(y, month, day) {
        if candidate < today {
            return y;
        }
    }
    y - 1
}

fn folder_for_date(base: &Path, date: NaiveDate) -> PathBuf {
    base.join(year_folder_name(date)).join(month_folder_name(date))
}

/// Среди файлов в папке выбирает реестр с максимальной датой строго меньше `today`.
pub fn pick_latest_before_today(dir: &Path, today: NaiveDate) -> Result<PathBuf> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("не удалось прочитать каталог {}", dir.display()))?;

    let mut best: Option<(NaiveDate, PathBuf)> = None;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((day, month)) = parse_register_filename(name) else {
            continue;
        };
        let year = year_for_day_month(day, month, today);
        let Some(file_date) = NaiveDate::from_ymd_opt(year, month, day) else {
            continue;
        };
        if file_date >= today {
            continue;
        }

        match &best {
            Some((best_date, _)) if file_date <= *best_date => {}
            _ => best = Some((file_date, path)),
        }
    }

    best.map(|(_, path)| path).with_context(|| {
        format!(
            "в {} нет файлов реестра с датой раньше {}",
            dir.display(),
            today
        )
    })
}

/// Автопоиск реестра на UNC по правилам плана.
pub fn discover_latest(base_unc: &str, today: NaiveDate) -> Result<PathBuf> {
    let target = target_register_date(today);
    let base = PathBuf::from(base_unc);
    let dir = folder_for_date(&base, target);

    if !dir.is_dir() {
        bail!(
            "каталог реестров не найден: {} (целевая дата {})",
            dir.display(),
            target
        );
    }

    tracing::info!(
        "поиск реестра в {} (целевая дата {}, сегодня {})",
        dir.display(),
        target,
        today
    );

    let path = pick_latest_before_today(&dir, today)?;
    tracing::info!("выбран файл {}", path.display());
    Ok(path)
}

/// Поиск файла по имени для `--file`.
pub fn resolve_by_filename(base_unc: &str, filename: &str, today: NaiveDate) -> Result<PathBuf> {
    let base = PathBuf::from(base_unc);
    let filename = filename.trim();
    if filename.is_empty() {
        bail!("имя файла пустое");
    }

    if let Some((day, month)) = parse_register_filename(filename) {
        let year = year_for_day_month(day, month, today);
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
            let candidate = folder_for_date(&base, date).join(filename);
            if candidate.is_file() {
                tracing::info!("файл найден по дате из имени: {}", candidate.display());
                return Ok(candidate);
            }
            tracing::warn!(
                "ожидаемый путь не найден ({}), пробуем обход каталогов",
                candidate.display()
            );
        }
    }

    // Fallback: обход папок текущего и предыдущего года.
    for year_offset in [0, 1] {
        let year = today.year() - year_offset;
        let year_dir = base.join(year.to_string());
        if !year_dir.is_dir() {
            continue;
        }
        for month in 1..=12 {
            let Some(date) = NaiveDate::from_ymd_opt(year, month, 1) else {
                continue;
            };
            let dir = year_dir.join(month_folder_name(date));
            let candidate = dir.join(filename);
            if candidate.is_file() {
                tracing::info!("файл найден обходом: {}", candidate.display());
                return Ok(candidate);
            }
        }
    }

    bail!(
        "файл '{}' не найден под базой {}",
        filename,
        base.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn month_folder_july_26() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 22).unwrap();
        assert_eq!(month_folder_name(d), "Июль 26");
        assert_eq!(year_folder_name(d), "2026");
    }

    #[test]
    fn target_date_on_first_of_month() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let target = target_register_date(today);
        assert_eq!(target, NaiveDate::from_ymd_opt(2026, 7, 31).unwrap());
        assert_eq!(month_folder_name(target), "Июль 26");
    }

    #[test]
    fn target_date_on_new_year() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let target = target_register_date(today);
        assert_eq!(target, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
        assert_eq!(year_folder_name(target), "2025");
        assert_eq!(month_folder_name(target), "Декабрь 25");
    }

    #[test]
    fn parse_double_dot_xls() {
        assert_eq!(
            parse_register_filename("Реестр 22.07..xls"),
            Some((22, 7))
        );
        assert_eq!(parse_register_filename("Реестр 1.12.xls"), Some((1, 12)));
        assert_eq!(parse_register_filename("readme.txt"), None);
    }

    #[test]
    fn year_for_december_in_january() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        assert_eq!(year_for_day_month(31, 12, today), 2025);
        assert_eq!(year_for_day_month(4, 1, today), 2026);
    }

    #[test]
    fn pick_latest_before_today_from_tmp_dir() {
        let dir = tempfile_dir();
        fs::write(dir.join("Реестр 20.07..xls"), b"a").unwrap();
        fs::write(dir.join("Реестр 22.07..xls"), b"b").unwrap();
        fs::write(dir.join("Реестр 23.07..xls"), b"c").unwrap();
        fs::write(dir.join("notes.txt"), b"x").unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 7, 23).unwrap();
        let picked = pick_latest_before_today(&dir, today).unwrap();
        assert_eq!(
            picked.file_name().unwrap().to_str().unwrap(),
            "Реестр 22.07..xls"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "okpo-discover-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
