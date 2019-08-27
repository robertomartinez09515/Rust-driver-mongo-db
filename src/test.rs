extern crate serde;
extern crate serde_json;

use std::{
    ffi::OsStr,
    fs::{self, File},
    path::PathBuf,
};

use bson::Bson;
use lazy_static::lazy_static;
use serde::Deserialize;
use serde_json::Value;

use crate::options::ClientOptions;

lazy_static! {
    pub static ref CLIENT_OPTIONS: ClientOptions = {
        let uri = option_env!("MONGODB_URI").unwrap_or("mongodb://localhost:27017");
        let mut options = ClientOptions::parse(uri).unwrap();
        options.max_pool_size = Some(100);

        options
    };
}

pub fn run<'a, T, F>(spec: &[&str], run_test_file: F)
where
    F: Fn(T),
    T: Deserialize<'a>,
{
    let base_path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "spec", "json"]
        .iter()
        .chain(spec.iter())
        .collect();

    for entry in fs::read_dir(&base_path).unwrap() {
        let test_file = entry.unwrap();

        if !test_file.file_type().unwrap().is_file() {
            continue;
        }

        let test_file_path = PathBuf::from(test_file.file_name());
        if test_file_path.extension().and_then(OsStr::to_str) != Some("json") {
            continue;
        }

        println!("file: {}", test_file_path.display());

        let test_file_full_path = base_path.join(&test_file_path);
        let json: Value =
            serde_json::from_reader(File::open(test_file_full_path.as_path()).unwrap()).unwrap();

        run_test_file(bson::from_bson(Bson::from(json)).unwrap())
    }
}
