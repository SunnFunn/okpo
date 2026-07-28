import pyodbc
import pandas as pd
from datetime import datetime
from dateutil.relativedelta import relativedelta

# servers and DBs
# /*ИСУ ПВ*/MSKOM1.OptimizerV2
# /*ИСУ ПВ*/MSKOM1.Dprognoz
# /*АСУ ВП*/MSKASUVPL.ASUVP_RAT
# /*RailTariff*/MSKASUVPL.RAT_RailTariff
# /*РАТ Онлайн*/MSKASUVPL.SLP

driver = '{ODBC Driver 18 for SQL Server}'
server = 'MSKASUVPL'
database = 'ASUVP_RAT'

def registers():
    start_date = (datetime.now() + relativedelta(days=-5)).strftime('%Y-%m-%d')

    # 2. Подключение и запрос (без изменений)
    connection_string = (
        'Trusted_Connection=yes;'
        f'Driver={driver};'
        f'Server={server};'
        f'Database={database};'
        'TrustServerCertificate=yes;'
        'MultipleActiveResultSets=True;'
    )

    conn = pyodbc.connect(connection_string)

    stmt= \
    f'''
    SELECT
        CarId, CarNumber, StationToId, SenderId, RecipId, RoadName, ShortName, StationTo, StationToCode
    FROM (
        SELECT
            RDC.CarId, DP.CarNumber, RDC.StationToId, RDC.SenderId, RDC.RecipId, RT.ShortName AS RoadName, F.ShortName, ST.Name AS StationTo, ST.Code6 AS StationToCode,
            RDC_Order = ROW_NUMBER() OVER (PARTITION BY RDC.CarId ORDER BY ISNULL(RDC.UpdatedDateTime, RDC.CreatedDateTime) DESC)
        FROM RegisterOfDestinationCar RDC (NOLOCK)
            JOIN DislocationPreview DP (NOLOCK) ON DP.CarId = RDC.CarId
            JOIN RegisterOfDestination RD (NOLOCK) ON RDC.RegisterOfDestinationId = RD.RegisterOfDestinationId
            JOIN Firm F (NOLOCK) ON RDC.RecipId = F.FirmId
            JOIN NSI.Station ST (NOLOCK) ON ST.StationId = RDC.StationToId
            JOIN NSI.RailWay RT (NOLOCK) ON RT.RailWayId = ST.RailWayId
        WHERE RDC.DateLink >= '{start_date}'
            AND RDC.OperationType = 'PP'
            AND RDC.StationToId != DP.StationToId
            AND DP.StationToId = RDC.StationFromId
        ) R1
    WHERE R1.RDC_Order = 1
    '''

    conn.close()


def invoices():
    limit_date = (datetime.now() + relativedelta(days=-60)).strftime('%Y-%m-%d')

    # 2. Подключение и запрос (без изменений)
    connection_string = (
        'Trusted_Connection=yes;'
        f'Driver={driver};'
        f'Server={server};'
        f'Database={database};'
        'TrustServerCertificate=yes;'
        'MultipleActiveResultSets=True;'
    )

    conn = pyodbc.connect(connection_string)

    stmt= \
    f'''
    SELECT BelongType, CarNumber,invNumber, StationFrom, StationFromCode, StationTo, StationToCode, RailWayTo, StateId
    FROM
        (
        SELECT
            *,
            ROW_NUMBER() OVER (PARTITION BY c.CarNumber ORDER BY c.Date DESC) AS Cars -- сортируем накладные по дате
        FROM
            (
            SELECT
                INC.CarNumber,
                IND.invNumber, SF.ST_NAME AS StationFrom, SF.ST_CODE6 AS StationFromCode,
                ST.ST_NAME AS StationTo, ST.ST_CODE6 AS StationToCode, RT.ShortName AS RailWayTo, IND.LastOper,
                IND.DateCreate, IND.IsLastVersion, IND.StateId, IND.TranspPurposeID, DP.StationToCode AS DislStationToCode, DP.BelongType,
                MIN(IND.LastOper) OVER (PARTITION BY INC.CarNumber, IND.invNumber) AS Date
            FROM  ETRAN.InvoiceDetail IND (NOLOCK)
                JOIN ETRAN.InvoiceCar INC (NOLOCK) ON INC.InvoiceDetailId = IND.InvoiceDetailId AND INC.IsDeleted = 0
                JOIN NSI_ETRAN.Station SF (NOLOCK) ON SF.ID = IND.StationFromId
                JOIN NSI_ETRAN.Station ST (NOLOCK) ON ST.ID = IND.StationToId
                JOIN DislocationPreview DP (NOLOCK) ON DP.CarNumber = INC.CarNumber
                JOIN NSI.Station S (NOLOCK) ON S.Code6 = ST.ST_CODE6
                JOIN NSI.RailWay RT (NOLOCK) ON RT.RailWayId = S.RailWayId
            WHERE IND.LastOper >= '{limit_date}'
            AND IND.IsDeleted = 0
            AND SF.ST_CODE6 != ST.ST_CODE6 -- исключаем внутристанционные накладные
            AND ST.ST_CODE6 != DP.StationToCode
            AND SF.ST_CODE6 = DP.StationToCode
            AND IND.TranspPurposeID = 1
            --AND IND.IsLastVersion = 1 -- необходимо брать все версии накладной, чтобы найти самую первую!
            -- IND.DateCreate >= ''
            ) c
        ) CLEARED
    WHERE CLEARED.Cars = 1
    AND CLEARED.StateId IN (SELECT STATE FROM NSI_ETRAN.DocState (NOLOCK) WHERE FOLDER != 'Не действительны' AND DOC_TYPE_ID = 2 AND IsDeleted = 0)
    '''

    conn.close()

if __name__ == "__main__":
    # registers()
    invoices()
