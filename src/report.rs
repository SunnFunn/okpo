//! Excel-отчёт для режима `--test-check`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rust_xlsxwriter::{Format, Workbook, Worksheet};

const HEADERS: [&str; 8] = [
    "Станция отправления",
    "Код станции отправления",
    "Дорога отправления",
    "Станция назначения",
    "Код станции назначения",
    "Дорога назначения",
    "Источник",
    "Количество вагонов",
];

const COL_WIDTHS: [f64; 8] = [28.0, 18.0, 14.0, 28.0, 18.0, 14.0, 16.0, 18.0];

/// Строка до агрегации (номерной вагон или пакет NoNumber).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    pub car_number: String,
    pub station_from: String,
    pub station_from_code: String,
    pub railway_from: String,
    pub station_to: String,
    pub station_to_code: String,
    pub railway_to: String,
    pub source: &'static str,
    /// Для номерных — обычно 1; для `NoNumber` — `CarCount` из JSON.
    pub car_count: u32,
}

/// Строка Excel после группировки по маршруту/источнику.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregatedRow {
    pub station_from: String,
    pub station_from_code: String,
    pub railway_from: String,
    pub station_to: String,
    pub station_to_code: String,
    pub railway_to: String,
    pub source: &'static str,
    pub car_count: u32,
}

type GroupKey = (String, String, String, String, String, String, &'static str);

/// Группирует строки: сумма `car_count` по маршруту + источнику.
pub fn aggregate_rows(rows: &[ReportRow]) -> Vec<AggregatedRow> {
    let mut counts: HashMap<GroupKey, u32> = HashMap::new();
    let mut order: Vec<GroupKey> = Vec::new();

    for row in rows {
        let key = (
            row.station_from.clone(),
            row.station_from_code.clone(),
            row.railway_from.clone(),
            row.station_to.clone(),
            row.station_to_code.clone(),
            row.railway_to.clone(),
            row.source,
        );
        let entry = counts.entry(key.clone()).or_insert(0);
        if *entry == 0 {
            order.push(key);
        }
        *entry = entry.saturating_add(row.car_count.max(1));
    }

    order
        .into_iter()
        .map(|key| {
            let car_count = *counts.get(&key).unwrap_or(&0);
            AggregatedRow {
                station_from: key.0,
                station_from_code: key.1,
                railway_from: key.2,
                station_to: key.3,
                station_to_code: key.4,
                railway_to: key.5,
                source: key.6,
                car_count,
            }
        })
        .collect()
}

/// Путь к отчёту: `{report_dir}/test_check_{YYYY-MM-DD}.xlsx`.
pub fn report_path(report_dir: &Path, export_date: NaiveDate) -> PathBuf {
    report_dir.join(format!(
        "test_check_{}.xlsx",
        export_date.format("%Y-%m-%d")
    ))
}

/// Пишет Excel-отчёт со сгруппированными строками.
pub fn write_report(path: &Path, rows: &[ReportRow]) -> Result<()> {
    let aggregated = aggregate_rows(rows);
    write_aggregated_report(path, &aggregated)
}

fn write_aggregated_report(path: &Path, rows: &[AggregatedRow]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("не удалось создать {}", parent.display()))?;
    }

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .set_name("Проверка")
        .context("имя листа Excel")?;

    let header_fmt = Format::new().set_bold();
    for (col, title) in HEADERS.iter().enumerate() {
        worksheet
            .write_with_format(0, col as u16, *title, &header_fmt)
            .with_context(|| format!("заголовок колонки {col}"))?;
        worksheet
            .set_column_width(col as u16, COL_WIDTHS[col])
            .ok();
    }

    for (i, row) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        write_row(worksheet, r, row)?;
    }

    let last_row = rows.len() as u32;
    let last_col = (HEADERS.len() - 1) as u16;
    worksheet.set_freeze_panes(1, 0).ok();
    if last_row > 0 {
        worksheet
            .autofilter(0, 0, last_row, last_col)
            .context("autofilter")?;
    } else {
        worksheet.autofilter(0, 0, 0, last_col).context("autofilter")?;
    }

    workbook
        .save(path)
        .with_context(|| format!("не удалось сохранить {}", path.display()))?;

    let total_cars: u32 = rows.iter().map(|r| r.car_count).sum();
    tracing::info!(
        path = %path.display(),
        groups = rows.len(),
        cars = total_cars,
        "Excel-отчёт записан"
    );
    Ok(())
}

fn write_row(ws: &mut Worksheet, row: u32, data: &AggregatedRow) -> Result<()> {
    let texts = [
        data.station_from.as_str(),
        data.station_from_code.as_str(),
        data.railway_from.as_str(),
        data.station_to.as_str(),
        data.station_to_code.as_str(),
        data.railway_to.as_str(),
        data.source,
    ];
    for (col, value) in texts.iter().enumerate() {
        ws.write_string(row, col as u16, *value)
            .with_context(|| format!("ячейка row={row} col={col}"))?;
    }
    // Количество — числом, чтобы в Excel работала сумма/фильтр.
    ws.write_number(row, 7, data.car_count as f64)
        .with_context(|| format!("ячейка row={row} col=7"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(car: &str, from: &str, to: &str, source: &'static str) -> ReportRow {
        ReportRow {
            car_number: car.into(),
            station_from: from.into(),
            station_from_code: "1".into(),
            railway_from: "МСК".into(),
            station_to: to.into(),
            station_to_code: "2".into(),
            railway_to: "ЮВС".into(),
            source,
            car_count: 1,
        }
    }

    #[test]
    fn aggregates_same_route() {
        let rows = vec![
            row("11111111", "A", "B", "registers"),
            row("22222222", "A", "B", "registers"),
            row("33333333", "A", "C", "registers"),
            row("44444444", "A", "B", "invoices"),
        ];
        let agg = aggregate_rows(&rows);
        assert_eq!(agg.len(), 3);
        assert_eq!(agg[0].car_count, 2);
        assert_eq!(agg[0].station_from, "A");
        assert_eq!(agg[0].station_to, "B");
        assert_eq!(agg[0].source, "registers");
        assert_eq!(agg[1].car_count, 1);
        assert_eq!(agg[1].station_to, "C");
        assert_eq!(agg[2].source, "invoices");
        assert_eq!(agg[2].car_count, 1);
    }

    #[test]
    fn aggregates_nonumber_car_count() {
        let rows = vec![
            row("11111111", "A", "B", "register_export"),
            ReportRow {
                car_number: "NoNumber".into(),
                station_from: "A".into(),
                station_from_code: "1".into(),
                railway_from: "МСК".into(),
                station_to: "B".into(),
                station_to_code: "2".into(),
                railway_to: "ЮВС".into(),
                source: "register_export",
                car_count: 159,
            },
        ];
        let agg = aggregate_rows(&rows);
        assert_eq!(agg.len(), 1);
        assert_eq!(agg[0].car_count, 160);
    }
}
