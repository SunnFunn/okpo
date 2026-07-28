//! Режим `--test-check`: JSON с прод + SQL + дедупликация + Excel.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::assignments::{self, AssignmentRow, AssignmentsOutput};
use crate::config::Config;
use crate::report::{self, ReportRow};
use crate::transfer;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ExportRow {
    #[serde(rename = "CarNumber")]
    car_number: String,
    #[serde(rename = "StationFromName", default)]
    station_from_name: Option<String>,
    #[serde(rename = "StationFromCode", default)]
    station_from_code: Option<String>,
    #[serde(rename = "RailroadFromName", default)]
    railroad_from_name: Option<String>,
    #[serde(rename = "StationToName", default)]
    station_to_name: Option<String>,
    #[serde(rename = "StationToCode", default)]
    station_to_code: Option<String>,
    #[serde(rename = "RailroadToName", default)]
    railroad_to_name: Option<String>,
}

/// Нормализация номера вагона: только цифры.
pub fn normalize_car_number(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Валидный номер: ровно 8 цифр после нормализации.
pub fn is_valid_car_number(raw: &str) -> bool {
    let n = normalize_car_number(raw);
    n.len() == 8
}

fn opt_string(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

fn from_assignment(row: &AssignmentRow, source: &'static str) -> Option<ReportRow> {
    let car = normalize_car_number(&row.car_number);
    if car.len() != 8 {
        return None;
    }
    Some(ReportRow {
        car_number: car,
        station_from: opt_string(&row.station_from),
        station_from_code: opt_string(&row.station_from_code),
        railway_from: opt_string(&row.railway_from),
        station_to: opt_string(&row.station_to),
        station_to_code: opt_string(&row.station_to_code),
        railway_to: opt_string(&row.railway_to),
        source,
    })
}

fn from_export(row: &ExportRow) -> Option<ReportRow> {
    let raw = row.car_number.trim();
    if raw.eq_ignore_ascii_case("NoNumber") || raw.eq_ignore_ascii_case("Unknown") {
        return None;
    }
    let car = normalize_car_number(raw);
    if car.len() != 8 {
        return None;
    }
    Some(ReportRow {
        car_number: car,
        station_from: opt_string(&row.station_from_name),
        station_from_code: opt_string(&row.station_from_code),
        railway_from: opt_string(&row.railroad_from_name),
        station_to: opt_string(&row.station_to_name),
        station_to_code: opt_string(&row.station_to_code),
        railway_to: opt_string(&row.railroad_to_name),
        source: "register_export",
    })
}

/// Дедупликация: registers → invoices → register_export (первое вхождение выигрывает).
pub fn build_report_rows(
    assignments: &AssignmentsOutput,
    export_rows: &[ExportRow],
) -> Vec<ReportRow> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<ReportRow> = Vec::new();
    let mut dropped_dup = 0usize;
    let mut dropped_invalid = 0usize;

    let reg_in = assignments.registers.len();
    for row in &assignments.registers {
        match from_assignment(row, "registers") {
            None => dropped_invalid += 1,
            Some(row) if !seen.insert(row.car_number.clone()) => dropped_dup += 1,
            Some(row) => out.push(row),
        }
    }
    let after_reg = out.len();

    let inv_in = assignments.invoices.len();
    for row in &assignments.invoices {
        match from_assignment(row, "invoices") {
            None => dropped_invalid += 1,
            Some(row) if !seen.insert(row.car_number.clone()) => dropped_dup += 1,
            Some(row) => out.push(row),
        }
    }
    let after_inv = out.len();

    let exp_in = export_rows.len();
    for row in export_rows {
        match from_export(row) {
            None => dropped_invalid += 1,
            Some(row) if !seen.insert(row.car_number.clone()) => dropped_dup += 1,
            Some(row) => out.push(row),
        }
    }

    tracing::info!(
        registers_in = reg_in,
        registers_kept = after_reg,
        invoices_in = inv_in,
        invoices_added = after_inv.saturating_sub(after_reg),
        export_in = exp_in,
        export_added = out.len().saturating_sub(after_inv),
        dropped_duplicates = dropped_dup,
        dropped_invalid,
        total = out.len(),
        "дедупликация завершена"
    );

    out
}

fn load_export_json(path: &Path) -> Result<Vec<ExportRow>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let rows: Vec<ExportRow> = serde_json::from_str(&text)
        .with_context(|| format!("невалидный JSON экспорта {}", path.display()))?;
    Ok(rows)
}

/// Полный прогон `--test-check`.
pub async fn run(cfg: &Config) -> Result<()> {
    tracing::info!("шаг 1/4: скачивание свежего JSON-экспорта с Ubuntu");
    let (local_json, export_date) =
        transfer::download_latest_export(&cfg.ssh, &cfg.test_check).await?;

    tracing::info!("шаг 2/4: SQL-запросы через assignments.py");
    let tc = cfg.test_check.clone();
    let assignments = tokio::task::spawn_blocking(move || assignments::fetch_assignments(&tc))
        .await
        .context("spawn_blocking assignments")??;

    tracing::info!("шаг 3/4: дедупликация registers + invoices + JSON");
    let export_rows = load_export_json(&local_json)?;
    let report_rows = build_report_rows(&assignments, &export_rows);

    tracing::info!("шаг 4/4: запись Excel-отчёта (группировка по маршруту)");
    let report_dir = if Path::new(&cfg.test_check.report_dir).is_absolute() {
        Path::new(&cfg.test_check.report_dir).to_path_buf()
    } else {
        Config::project_root().join(&cfg.test_check.report_dir)
    };
    let out_path = report::report_path(&report_dir, export_date);
    report::write_report(&out_path, &report_rows)?;

    let groups = report::aggregate_rows(&report_rows);
    let total_cars: u32 = groups.iter().map(|g| g.car_count).sum();
    tracing::info!(
        json = %local_json.display(),
        report = %out_path.display(),
        unique_wagons = report_rows.len(),
        groups = groups.len(),
        cars_in_report = total_cars,
        "--test-check завершён успешно"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assignments::AssignmentsOutput;

    fn assign(
        car: &str,
        from: &str,
        from_code: &str,
        rw_from: &str,
        to: &str,
        to_code: &str,
        rw_to: &str,
    ) -> AssignmentRow {
        AssignmentRow {
            car_number: car.into(),
            station_from: Some(from.into()),
            station_from_code: Some(from_code.into()),
            railway_from: Some(rw_from.into()),
            station_to: Some(to.into()),
            station_to_code: Some(to_code.into()),
            railway_to: Some(rw_to.into()),
        }
    }

    fn export(car: &str) -> ExportRow {
        ExportRow {
            car_number: car.into(),
            station_from_name: Some("JSON_FROM".into()),
            station_from_code: Some("111111".into()),
            railroad_from_name: Some("МСК".into()),
            station_to_name: Some("JSON_TO".into()),
            station_to_code: Some("222222".into()),
            railroad_to_name: Some("ЮВС".into()),
        }
    }

    #[test]
    fn normalize_and_validate() {
        assert_eq!(normalize_car_number(" 95-569-661 "), "95569661");
        assert!(is_valid_car_number("95569661"));
        assert!(!is_valid_car_number("NoNumber"));
        assert!(!is_valid_car_number("123"));
    }

    #[test]
    fn dedup_priority_registers_over_invoices_and_json() {
        let assignments = AssignmentsOutput {
            registers: vec![assign(
                "95569661", "A", "1", "МСК", "B", "2", "ЮВС",
            )],
            invoices: vec![assign(
                "95569661", "X", "9", "МСК", "Y", "8", "ЮВС",
            )],
        };
        let exports = vec![export("95569661"), export("95113130"), export("NoNumber")];
        let rows = build_report_rows(&assignments, &exports);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].car_number, "95569661");
        assert_eq!(rows[0].source, "registers");
        assert_eq!(rows[0].station_from, "A");
        assert_eq!(rows[1].car_number, "95113130");
        assert_eq!(rows[1].source, "register_export");
    }

    #[test]
    fn dedup_within_json_and_skip_unknown() {
        let assignments = AssignmentsOutput {
            registers: vec![],
            invoices: vec![],
        };
        let exports = vec![
            export("95113130"),
            export("95113130"),
            export("Unknown"),
            export("NoNumber"),
        ];
        let rows = build_report_rows(&assignments, &exports);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].car_number, "95113130");
    }

    #[test]
    fn invoices_added_when_not_in_registers() {
        let assignments = AssignmentsOutput {
            registers: vec![assign(
                "11111111", "A", "1", "МСК", "B", "2", "ЮВС",
            )],
            invoices: vec![assign(
                "22222222", "C", "3", "МСК", "D", "4", "ЮВС",
            )],
        };
        let rows = build_report_rows(&assignments, &[]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].source, "invoices");
    }
}
