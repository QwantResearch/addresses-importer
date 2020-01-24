// TODO
//  - filtrer les addresses en entrée pour la France
//  - stocker un identifiant de la source dans la BDD et les prioriser (envoyer un closure à
//    compute_duplicates)

extern crate geo;
extern crate geo_geojson;
extern crate r2d2;
extern crate r2d2_sqlite;
extern crate rayon;
extern crate rpostal;
extern crate rprogress;
extern crate rusqlite;
extern crate structopt;

#[macro_use]
mod address;
mod db_hashes;
mod dedupe;
mod postal_wrappers;

use std::convert::TryFrom;
use std::fs::read_to_string;
use std::path::PathBuf;

use geo::algorithm::contains::Contains;
use geo::{MultiPolygon, Point};
use rprogress::prelude::*;
use rusqlite::{Connection, NO_PARAMS};
use structopt::StructOpt;

use address::Address;
use dedupe::Dedupe;

const FRANCE_GEOJSON: &str = "data/france.json";

const PRIORITY_OSM: f32 = 2.;
const PRIORITY_OPENADDRESS: f32 = 1.;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "deduplicator",
    about = "Deduplicate addresses from several sources."
)]
struct Params {
    /// Path to data from OpenAddress as an SQLite database
    #[structopt(short = "a", long)]
    openaddress_db: Vec<PathBuf>,

    /// Path to data from OSM as an SQLite database
    #[structopt(short = "s", long)]
    osm_db: Vec<PathBuf>,

    /// Path to output database.
    #[structopt(short, long, default_value = "addresses.db")]
    output: PathBuf,
}

fn load_from_sqlite<F, R>(
    deduplication: &mut Dedupe,
    path: PathBuf,
    filter: F,
    _ranking: R,
) -> rusqlite::Result<()>
where
    F: Fn(&Address) -> bool,
    R: Fn(&Address) -> f32,
{
    let input_conn = Connection::open(&path)?;
    let nb_addresses = usize::try_from(input_conn.query_row(
        "SELECT COUNT(*) FROM addresses;",
        NO_PARAMS,
        |row| row.get(0).map(|x: isize| x),
    )?)
    .unwrap();

    eprintln!("Importing {} addresses from {:?}", nb_addresses, path);

    let mut stmt = input_conn.prepare("SELECT * FROM addresses")?;
    let addresses = stmt
        .query_map(NO_PARAMS, |row| Address::from_sqlite_row(&row))?
        .map(|addr| addr.unwrap_or_else(|e| panic!("failed to read address from source: {}", e)))
        .uprogress(nb_addresses)
        .with_prefix(format!("Importing {:<55}", format!("{:?}", path)))
        .filter(filter);

    deduplication.load_addresses(addresses)
}

fn main() -> rusqlite::Result<()> {
    // --- Load France filter

    let france_shape: MultiPolygon<_> = {
        let geojson = read_to_string(FRANCE_GEOJSON).expect("failed to load shape for France");
        let collection = geo_geojson::from_str(&geojson).expect("failed to parse shape for France");
        collection
            .into_iter()
            .next()
            .unwrap()
            .into_multi_polygon()
            .unwrap()
    };

    let is_in_france = move |address: &Address| -> bool {
        france_shape.contains(&Point::new(address.lon, address.lat))
    };

    // --- Read parameters
    let params = Params::from_args();
    let mut deduplication = Dedupe::new(params.output)?;

    // --- Read database from various sources

    println!(
        "Loading OSM addresses from {} SQLite databases",
        params.osm_db.len()
    );

    for path in params.osm_db {
        load_from_sqlite(
            &mut deduplication,
            path,
            |addr| !is_in_france(addr),
            |_| PRIORITY_OSM,
        )
        .expect("failed to load addresses from database");
    }

    println!(
        "Loading OpenAddress addresses from {} SQLite databases",
        params.openaddress_db.len()
    );

    for path in params.openaddress_db {
        load_from_sqlite(&mut deduplication, path, |_| true, |_| PRIORITY_OPENADDRESS)
            .expect("failed to load addresses from database");
    }

    // --- Apply deduplication

    deduplication.compute_duplicates()?;
    deduplication.apply_and_clean()?;

    Ok(())
}
