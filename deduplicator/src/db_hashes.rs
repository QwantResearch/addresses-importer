use rusqlite::{Connection, Statement, ToSql, NO_PARAMS};

use crate::address::Address;

pub struct DbHashes<'c> {
    conn: &'c Connection,
    stmt_insert_address: Statement<'c>,
    stmt_insert_hash: Statement<'c>,
}

impl<'c> DbHashes<'c> {
    pub fn new(conn: &'c Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS addresses(
                lat         REAL NOT NULL,
                lon         REAL NOT NULL,
                number      TEXT,
                street      TEXT NOT NULL,
                unit        TEXT,
                city        TEXT,
                district    TEXT,
                region      TEXT,
                postcode    TEXT,
                PRIMARY KEY (lat, lon, number, street, city)
            );
            DELETE FROM addresses;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS addresses_hashes(
                address     INTEGER NOT NULL,
                hash        INTEGER NOT NULL
            );
            DELETE FROM addresses_hashes;",
        )?;

        Ok(Self {
            conn,
            stmt_insert_address: conn.prepare(
                "INSERT INTO addresses(
                    lat,
                    lon,
                    number,
                    street,
                    unit,
                    city,
                    district,
                    region,
                    postcode
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9);",
            )?,

            stmt_insert_hash: conn
                .prepare("INSERT INTO addresses_hashes(address, hash) VALUES (?1, ?2);")?,
        })
    }

    pub fn begin_transaction(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch("BEGIN TRANSACTION;")
    }

    pub fn commit_transaction(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch("COMMIT TRANSACTION;")
    }

    pub fn insert_address(&mut self, address: &Address) -> rusqlite::Result<i64> {
        self.stmt_insert_address.execute(&[
            &address.lat as &dyn ToSql,
            &address.lon,
            &address.number,
            &address.street,
            &address.unit,
            &address.city,
            &address.district,
            &address.region,
            &address.postcode,
        ])?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn insert_hash(&mut self, address_id: i64, hash: i64) -> rusqlite::Result<i64> {
        self.stmt_insert_hash.execute(&[address_id, hash])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn feasible_duplicates(&self) -> rusqlite::Result<CollisionsIterable<'c>> {
        CollisionsIterable::prepare(&self.conn)
    }
}

pub struct CollisionsIterable<'c>(Statement<'c>);

impl<'c> CollisionsIterable<'c> {
    pub fn prepare(conn: &'c Connection) -> rusqlite::Result<Self> {
        Ok(CollisionsIterable(conn.prepare(
            "
                SELECT DISTINCT
                    addr_1.rowid        AS addr_1_id,
                    addr_1.lat          AS addr_1_lat,
                    addr_1.lon          AS addr_1_lon,
                    addr_1.number       AS addr_1_number,
                    addr_1.street       AS addr_1_street,
                    addr_1.unit         AS addr_1_unit,
                    addr_1.city         AS addr_1_city,
                    addr_1.district     AS addr_1_district,
                    addr_1.region       AS addr_1_region,
                    addr_1.postcode     AS addr_1_postcode,
                    addr_2.rowid        AS addr_2_id,
                    addr_2.lat          AS addr_2_lat,
                    addr_2.lon          AS addr_2_lon,
                    addr_2.number       AS addr_2_number,
                    addr_2.street       AS addr_2_street,
                    addr_2.unit         AS addr_2_unit,
                    addr_2.city         AS addr_2_city,
                    addr_2.district     AS addr_2_district,
                    addr_2.region       AS addr_2_region,
                    addr_2.postcode     AS addr_2_postcode
                FROM addresses AS addr_1
                JOIN addresses AS addr_2
                JOIN addresses_hashes AS hash_1 ON addr_1.rowid = hash_1.address
                JOIN addresses_hashes AS hash_2 ON addr_2.rowid = hash_2.address
                WHERE addr_1.rowid < addr_2.rowid AND hash_1.hash = hash_2.hash;
            ",
        )?))
    }

    pub fn iter<'s>(
        &'s mut self,
    ) -> rusqlite::Result<
        impl Iterator<Item = rusqlite::Result<((i64, Address), (i64, Address))>> + 's,
    > {
        let Self(stmt) = self;

        Ok(stmt.query_map(NO_PARAMS, |row| {
            Ok((
                (
                    row.get("addr_1_id")?,
                    address_from_sqlite_row_with_prefix!("addr_1_", row)?,
                ),
                (
                    row.get("addr_1_id")?,
                    address_from_sqlite_row_with_prefix!("addr_2_", row)?,
                ),
            ))
        })?)
    }
}