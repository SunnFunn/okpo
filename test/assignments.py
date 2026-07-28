#!/usr/bin/env python3
"""
Два SQL-запроса: вагоны, едущие по дислокации на одну станцию,
но уже назначенные диспетчером под погрузку на другую.

stdout: JSON {"registers": [...], "invoices": [...]}
stderr: OKPO_ASSIGNMENTS_STATS={"registers":N,"invoices":M}

Строка результата (оба массива):
  CarNumber, StationFrom, StationFromCode, RailWayFrom,
  StationTo, StationToCode, RailWayTo
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timedelta
from typing import Any

import pyodbc

DRIVER = "{ODBC Driver 18 for SQL Server}"
SERVER = "MSKASUVPL"
DATABASE = "ASUVP_RAT"
STATS_PREFIX = "OKPO_ASSIGNMENTS_STATS="

REGISTERS_SQL = """
SELECT
    CarNumber, StationFrom, StationFromCode, RailWayFrom,
    StationTo, StationToCode, RailWayTo
FROM (
    SELECT
        DP.CarNumber,
        SF.Name AS StationFrom,
        SF.Code6 AS StationFromCode,
        RF.ShortName AS RailWayFrom,
        ST.Name AS StationTo,
        ST.Code6 AS StationToCode,
        RT.ShortName AS RailWayTo,
        RDC_Order = ROW_NUMBER() OVER (
            PARTITION BY RDC.CarId
            ORDER BY ISNULL(RDC.UpdatedDateTime, RDC.CreatedDateTime) DESC
        )
    FROM RegisterOfDestinationCar RDC (NOLOCK)
        JOIN DislocationPreview DP (NOLOCK) ON DP.CarId = RDC.CarId
        JOIN RegisterOfDestination RD (NOLOCK)
            ON RDC.RegisterOfDestinationId = RD.RegisterOfDestinationId
        JOIN Firm F (NOLOCK) ON RDC.RecipId = F.FirmId
        JOIN NSI.Station ST (NOLOCK) ON ST.StationId = RDC.StationToId
        JOIN NSI.RailWay RT (NOLOCK) ON RT.RailWayId = ST.RailWayId
        JOIN NSI.Station SF (NOLOCK) ON SF.StationId = RDC.StationFromId
        JOIN NSI.RailWay RF (NOLOCK) ON RF.RailWayId = SF.RailWayId
    WHERE RDC.DateLink >= ?
        AND RDC.OperationType = 'PP'
        AND RDC.StationToId != DP.StationToId
        AND DP.StationToId = RDC.StationFromId
) R1
WHERE R1.RDC_Order = 1
"""

INVOICES_SQL = """
SELECT
    CarNumber, StationFrom, StationFromCode, RailWayFrom,
    StationTo, StationToCode, RailWayTo
FROM (
    SELECT
        *,
        ROW_NUMBER() OVER (PARTITION BY c.CarNumber ORDER BY c.Date DESC) AS Cars
    FROM (
        SELECT
            INC.CarNumber,
            SF.ST_NAME AS StationFrom,
            SF.ST_CODE6 AS StationFromCode,
            RF.ShortName AS RailWayFrom,
            ST.ST_NAME AS StationTo,
            ST.ST_CODE6 AS StationToCode,
            RT.ShortName AS RailWayTo,
            IND.LastOper,
            IND.StateId,
            MIN(IND.LastOper) OVER (PARTITION BY INC.CarNumber, IND.invNumber) AS Date
        FROM ETRAN.InvoiceDetail IND (NOLOCK)
            JOIN ETRAN.InvoiceCar INC (NOLOCK)
                ON INC.InvoiceDetailId = IND.InvoiceDetailId AND INC.IsDeleted = 0
            JOIN NSI_ETRAN.Station SF (NOLOCK) ON SF.ID = IND.StationFromId
            JOIN NSI_ETRAN.Station ST (NOLOCK) ON ST.ID = IND.StationToId
            JOIN DislocationPreview DP (NOLOCK) ON DP.CarNumber = INC.CarNumber
            JOIN NSI.Station S (NOLOCK) ON S.Code6 = ST.ST_CODE6
            JOIN NSI.RailWay RT (NOLOCK) ON RT.RailWayId = S.RailWayId
            JOIN NSI.Station S_F (NOLOCK) ON S_F.Code6 = SF.ST_CODE6
            JOIN NSI.RailWay RF (NOLOCK) ON RF.RailWayId = S_F.RailWayId
        WHERE IND.LastOper >= ?
            AND IND.IsDeleted = 0
            AND SF.ST_CODE6 != ST.ST_CODE6
            AND ST.ST_CODE6 != DP.StationToCode
            AND SF.ST_CODE6 = DP.StationToCode
            AND IND.TranspPurposeID = 1
    ) c
) CLEARED
WHERE CLEARED.Cars = 1
AND CLEARED.StateId IN (
    SELECT STATE FROM NSI_ETRAN.DocState (NOLOCK)
    WHERE FOLDER != N'Не действительны' AND DOC_TYPE_ID = 2 AND IsDeleted = 0
)
"""


def connect() -> Any:
    connection_string = (
        "Trusted_Connection=yes;"
        f"Driver={DRIVER};"
        f"Server={SERVER};"
        f"Database={DATABASE};"
        "TrustServerCertificate=yes;"
        "MultipleActiveResultSets=True;"
    )
    return pyodbc.connect(connection_string)


def normalize_car_number(raw: Any) -> str:
    if raw is None or isinstance(raw, bool):
        return ""
    if isinstance(raw, int):
        return str(abs(raw))
    if isinstance(raw, float):
        return str(int(raw))
    return "".join(c for c in str(raw).strip() if c.isdigit())


def _cell(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def _row_to_dict(columns: list[str], row: Any) -> dict[str, str] | None:
    data = {columns[i]: row[i] for i in range(len(columns))}
    car = normalize_car_number(data.get("CarNumber"))
    if not car:
        return None
    return {
        "CarNumber": car,
        "StationFrom": _cell(data.get("StationFrom")),
        "StationFromCode": _cell(data.get("StationFromCode")),
        "RailWayFrom": _cell(data.get("RailWayFrom")),
        "StationTo": _cell(data.get("StationTo")),
        "StationToCode": _cell(data.get("StationToCode")),
        "RailWayTo": _cell(data.get("RailWayTo")),
    }


def fetch_rows(sql: str, param: str) -> list[dict[str, str]]:
    conn = connect()
    cur = conn.cursor()
    out: list[dict[str, str]] = []
    try:
        cur.execute(sql, (param,))
        columns = [col[0] for col in cur.description]
        for row in cur.fetchall():
            item = _row_to_dict(columns, row)
            if item is not None:
                out.append(item)
        return out
    finally:
        cur.close()
        conn.close()


def registers(days: int) -> list[dict[str, str]]:
    start_date = (datetime.now() + timedelta(days=-days)).strftime("%Y-%m-%d")
    return fetch_rows(REGISTERS_SQL, start_date)


def invoices(days: int) -> list[dict[str, str]]:
    limit_date = (datetime.now() + timedelta(days=-days)).strftime("%Y-%m-%d")
    return fetch_rows(INVOICES_SQL, limit_date)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Вагоны с расхождением дислокации и назначений (реестр + накладные)"
    )
    parser.add_argument("--registers-days", type=int, default=5)
    parser.add_argument("--invoices-days", type=int, default=60)
    args = parser.parse_args()

    try:
        regs = registers(args.registers_days)
        invs = invoices(args.invoices_days)
    except Exception as exc:  # noqa: BLE001 — отчёт в stderr для Rust-моста
        print(f"assignments: ошибка SQL: {exc}", file=sys.stderr)
        return 1

    stats = {"registers": len(regs), "invoices": len(invs)}
    print(f"{STATS_PREFIX}{json.dumps(stats, ensure_ascii=False)}", file=sys.stderr)

    payload = {"registers": regs, "invoices": invs}
    json.dump(payload, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
