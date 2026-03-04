use camino::Utf8PathBuf;
use std::env;
use uniffi_bindgen::bindings::{KotlinBindingGenerator, SwiftBindingGenerator};
use uniffi_bindgen::library_mode;

fn main() {
    if let Err(error) = run() {
        eprintln!("bindgen error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut language: Option<String> = None;
    let mut library: Option<Utf8PathBuf> = None;
    let mut out_dir: Option<Utf8PathBuf> = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--language" => language = args.next(),
            "--library" => library = args.next().map(Utf8PathBuf::from),
            "--out-dir" => out_dir = args.next().map(Utf8PathBuf::from),
            _ => {}
        }
    }

    let language = language.ok_or("missing --language")?;
    let library = library.ok_or("missing --library")?;
    let out_dir = out_dir.ok_or("missing --out-dir")?;

    let config_supplier = uniffi_bindgen::EmptyCrateConfigSupplier;

    match language.as_str() {
        "swift" => {
            let components = library_mode::generate_bindings(
                &library,
                None,
                &SwiftBindingGenerator,
                &config_supplier,
                None,
                &out_dir,
                false,
            )?;
            if components.is_empty() {
                return Err("no UniFFI components discovered in library".into());
            }
        }
        "kotlin" => {
            let components = library_mode::generate_bindings(
                &library,
                None,
                &KotlinBindingGenerator,
                &config_supplier,
                None,
                &out_dir,
                false,
            )?;
            if components.is_empty() {
                return Err("no UniFFI components discovered in library".into());
            }
        }
        _ => return Err(format!("unsupported language: {language}").into()),
    }

    Ok(())
}
