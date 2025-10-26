use crate::cli::Args;
use crate::config::Config;
use crate::services::{
    country::CountryService, database::DatabaseService, extraction::ExtractionService,
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

async fn download_and_decompress_database(
    config: &Config,
    compressed_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Downloading WhosOnFirst database...");
    crate::utils::file::download_file_with_progress(
        &config.whosonfirst_db_url,
        Path::new(compressed_path),
    )
    .await?;
    println!("Database download completed!");

    println!("Decompressing database...");
    let output =
        crate::utils::cmd::run_command(&config.bzip2_cmd, &["-dv", compressed_path], None).await?;

    if !output.stderr.is_empty() {
        eprintln!("Decompression output: {}", output.stderr);
    }

    println!("Database decompressed successfully!");
    Ok(())
}

async fn extract_missing_localities(
    extraction_service: &ExtractionService,
    results: Vec<(&String, &String, u32, u32, bool)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let missing_country_codes: Vec<String> = results
        .into_iter()
        .filter(|(_, _, _, _, is_complete)| !is_complete)
        .map(|(country_code, _, _, _, _)| (*country_code).clone())
        .collect();

    if !missing_country_codes.is_empty() {
        extraction_service
            .extract_localities(&missing_country_codes)
            .await?;
        println!("Extraction completed.");
    }
    Ok(())
}

pub async fn ensure_tools_are_present(tools: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    use crate::utils::cmd::ensure_tools_are_present as check_tools;

    check_tools(tools).await?;
    Ok(())
}

pub async fn ensure_database_is_present(
    config: &Config,
    args: &Args,
) -> Result<(), Box<dyn std::error::Error>> {
    let database_path = config.database_path();
    let compressed_path = format!("{}.bz2", database_path.display());

    if database_path.exists() {
        println!("Database already present.");
        return Ok(());
    }

    if Path::new(&compressed_path).exists() {
        println!("Compressed database found, decompressing...");

        let output =
            crate::utils::cmd::run_command(&config.bzip2_cmd, &["-dv", &compressed_path], None)
                .await?;

        if !output.stderr.is_empty() {
            eprintln!("Decompression output: {}", output.stderr);
        }

        println!("Database decompressed successfully!");
        return Ok(());
    }

    println!("WhosOnFirst database not found.");

    if args.should_download_database() {
        println!("Auto-downloading WhosOnFirst database...");
        download_and_decompress_database(config, &compressed_path).await?;
        return Ok(());
    } else if args.is_interactive_mode() {
        print!("Do you want to download the WhosOnFirst database? This may take a while. (y/n) ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if input.trim().to_lowercase() == "y" {
            download_and_decompress_database(config, &compressed_path).await?;
            return Ok(());
        }
    }
    println!("Database download skipped.");
    Err("Database is missing and download is disabled".into())
}

pub async fn ensure_all_localities_present(
    extraction_service: &ExtractionService,
    country_service: &CountryService,
    config: &Config,
    db_service: &DatabaseService,
    args: &Args,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Checking localities extraction status...");

    let countries_to_check = country_service.get_countries_to_process(&config.target_countries);

    if countries_to_check.is_empty() {
        println!("No countries to process");
        return Ok(());
    }

    println!("Counting pmtiles files...");
    let file_count_map = extraction_service
        .batch_get_pmtiles_file_count(&countries_to_check)
        .await?;

    println!("Querying database for locality counts...");
    let mut db_count_map = HashMap::new();

    for country_code in &countries_to_check {
        let db_count = db_service.get_country_locality_count(country_code).await?;
        db_count_map.insert(country_code.clone(), db_count);
    }

    let mut results = Vec::new();
    let mut all_complete = true;

    for country_code in &countries_to_check {
        let country_name = country_service
            .get_country_name(country_code)
            .unwrap_or(country_code);
        let db_count = db_count_map.get(country_code).unwrap_or(&0);
        let file_count = file_count_map.get(country_code).unwrap_or(&0);
        let is_complete = db_count == file_count;

        if !is_complete {
            all_complete = false;
        }

        results.push((
            country_code,
            country_name,
            *db_count,
            *file_count,
            is_complete,
        ));
    }

    if all_complete {
        println!("✓ All localities have been extracted!");
        return Ok(());
    }

    println!("Country Code | Country Name                  | DB Count | File Count | Status");
    println!("-------------|-------------------------------|----------|------------|--------");

    for (country_code, country_name, db_count, file_count, is_complete) in &results {
        let status = if *is_complete {
            "✓ Complete"
        } else {
            "✗ Incomplete"
        };
        let truncated_name = if country_name.len() > 29 {
            format!("{}...", &country_name[..26])
        } else {
            country_name.to_string()
        };
        println!(
            "{:12} | {:29} | {:8} | {:10} | {}",
            country_code, truncated_name, db_count, file_count, status
        );
    }

    println!("✗ Some localities are missing. Extraction is incomplete.");

    if args.should_extract_localities() {
        println!("Auto-extracting missing localities...");
        extract_missing_localities(extraction_service, results.clone()).await?;
        return Ok(());
    } else if args.is_interactive_mode() {
        // Interactive mode - prompt the user
        print!("Do you want to extract the missing localities? (y/n) ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().to_lowercase() == "y" {
            extract_missing_localities(extraction_service, results.clone()).await?;
            return Ok(());
        }
    }
    println!("Extraction skipped.");
    Ok(())
}
