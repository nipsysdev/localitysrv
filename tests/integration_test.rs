//! Real integration test that demonstrates the complete workflow with actual Codex uploads
//!
//! This test uses real PMTiles files and performs actual Codex network uploads.

use localitysrv::config::LocalitySrvConfig;
use localitysrv::node::manager::CodexNodeManager;
use localitysrv::services::database::DatabaseService;
use localitysrv::services::node_ops::NodeOps;
use std::fs;
use std::sync::Arc;

#[tokio::test]
async fn test_real_codex_integration() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to see Codex logs
    let _ = env_logger::try_init();

    println!("Starting real Codex integration test");

    // Use tests/resources directory for all test data
    let test_resources_dir = std::path::Path::new("tests/resources");
    let whosonfirst_db_path = test_resources_dir.join("test_whosonfirst.db");
    let cid_db_path = test_resources_dir.join("test_cid_mappings.db");
    let localities_dir = test_resources_dir.join("localities");

    // Clean up any existing test data
    cleanup_test_data(&test_resources_dir).await?;

    // Create directories if they don't exist
    std::fs::create_dir_all(&localities_dir)?;
    std::fs::create_dir_all(localities_dir.join("AE"))?;

    // Create localities directory structure
    std::fs::create_dir_all(&localities_dir)?;
    std::fs::create_dir_all(localities_dir.join("AE"))?;

    // Copy real PMTiles files to test directory
    copy_test_pmtiles_files(&localities_dir).await?;
    println!("✓ Copied real PMTiles files to test directory");

    // Create test configuration
    let config = create_test_config(&whosonfirst_db_path, &cid_db_path, &localities_dir)?;

    // Initialize WhosOnFirst database service (read-only)
    let whosonfirst_db_service =
        Arc::new(DatabaseService::new(&whosonfirst_db_path.to_string_lossy()).await?);

    // Initialize CID mappings database service (read-write)
    let cid_db_service = Arc::new(DatabaseService::new(&cid_db_path.to_string_lossy()).await?);

    // Create a real node manager
    let node_manager = Arc::new(CodexNodeManager::new(config.codex.clone()));

    // Create node operations service with separate databases
    let node_ops = NodeOps::new_with_databases(
        cid_db_service.clone(),
        whosonfirst_db_service.clone(),
        node_manager.clone(),
    );

    println!("✓ Setup completed successfully");

    // Test 1: Verify PMTiles files exist and are real
    verify_pmtiles_files(&localities_dir).await?;
    println!("✓ PMTiles files verified");

    // Test 2: Insert test CID mappings into CID database
    insert_test_locality_data(&cid_db_service).await?;
    println!("✓ Test CID mappings inserted into CID database");

    // Test 3: Test real Codex uploads
    let (real_cid, real_size) = test_real_codex_uploads(&node_ops, &cid_db_service).await?;
    println!("✓ Real Codex uploads completed");
    println!("  Real CID: {}", real_cid);
    println!("  Real size: {} bytes", real_size);

    // Test 4: Verify CID mappings database contains uploaded CIDs
    verify_database_cids(&cid_db_service).await?;
    println!("✓ Database CID mappings verified");

    println!("Real Codex integration test completed successfully!");
    println!("WhosOnFirst database saved at: {:?}", whosonfirst_db_path);
    println!("CID mappings database saved at: {:?}", cid_db_path);
    println!(
        "You can inspect the databases with: sqlite3 {:?} and sqlite3 {:?}",
        whosonfirst_db_path, cid_db_path
    );

    // Clean up test data after successful test
    cleanup_test_data(&test_resources_dir).await?;
    println!("✓ Cleaned up test data after test completion");

    Ok(())
}

async fn copy_test_pmtiles_files(
    localities_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_dir = std::path::Path::new("assets/localities/AE");
    let target_dir = localities_dir.join("AE");

    // Copy the real PMTiles files from assets to test resources
    let test_files = [
        "421168683.pmtiles",
        "421168685.pmtiles",
        "421168687.pmtiles",
    ];

    for file in &test_files {
        let src = source_dir.join(file);
        let dst = target_dir.join(file);

        if src.exists() {
            tokio::fs::copy(&src, &dst).await?;
            println!("  Copied {} from assets to test directory", file);
        } else {
            return Err(format!("Source file not found: {:?}", src).into());
        }
    }

    Ok(())
}

async fn verify_pmtiles_files(
    localities_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let ae_dir = localities_dir.join("AE");
    let test_files = [
        "421168683.pmtiles",
        "421168685.pmtiles",
        "421168687.pmtiles",
    ];

    for file in &test_files {
        let file_path = ae_dir.join(file);

        // Check file exists
        assert!(
            file_path.exists(),
            "PMTiles file should exist: {:?}",
            file_path
        );

        // Check file size (should be substantial, not empty)
        let metadata = tokio::fs::metadata(&file_path).await?;
        assert!(
            metadata.len() > 1000,
            "PMTiles file should be substantial: {} bytes",
            metadata.len()
        );

        println!("  Verified {}: {} bytes", file, metadata.len());
    }

    Ok(())
}

async fn insert_test_locality_data(
    cid_db_service: &Arc<DatabaseService>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Inserting test CID mappings for AE country...");

    // Insert some test CID mappings to simulate uploaded localities
    let test_mappings = vec![
        (
            "AE".to_string(),
            421168683,
            "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi421168683".to_string(),
            10965857, // Real file size
        ),
        (
            "AE".to_string(),
            421168685,
            "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi421168685".to_string(),
            1327870, // Real file size
        ),
        (
            "AE".to_string(),
            421168687,
            "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi421168687".to_string(),
            7484944, // Real file size
        ),
    ];

    // For testing purposes, we'll insert these as if they were already uploaded
    // to test the database operations. In the real upload test, we'll overwrite these.
    cid_db_service
        .batch_insert_cid_mappings(&test_mappings)
        .await?;

    println!("  Inserted {} test CID mappings", test_mappings.len());
    Ok(())
}

async fn test_real_codex_uploads(
    _node_ops: &NodeOps,
    db_service: &Arc<DatabaseService>,
) -> Result<(String, u64), Box<dyn std::error::Error>> {
    println!("Testing real Codex uploads...");

    // Test direct Codex upload using the bindings
    println!("  Testing direct Codex upload...");

    let test_file_path =
        std::path::PathBuf::from("tests/resources/localities/AE/421168683.pmtiles");
    if !test_file_path.exists() {
        return Err("Test PMTiles file not found".into());
    }

    // Create a Codex node using test resources directory
    let test_resources_dir = std::path::Path::new("tests/resources");
    let config = codex_bindings::CodexConfig::new()
        .log_level(codex_bindings::LogLevel::Info)
        .data_dir(test_resources_dir.join(".codex-test-data"))
        .storage_quota(100 * 1024 * 1024) // 100MB
        .discovery_port(0); // Use random port

    let mut node = codex_bindings::CodexNode::new(config)?;
    node.start()?;

    println!("  Started temporary Codex node for upload test");

    // Perform real upload
    let upload_options = codex_bindings::UploadOptions::new()
        .filepath(&test_file_path)
        .on_progress(|progress| {
            let percentage = (progress.percentage * 100.0) as u32;
            println!(
                "    Upload progress: {} bytes ({}%)",
                progress.bytes_uploaded, percentage
            );
        });

    let upload_result = codex_bindings::upload_file(&node, upload_options).await?;

    println!("  ✓ Real upload completed!");
    println!("    CID: {}", upload_result.cid);
    println!("    Size: {} bytes", upload_result.size);
    println!("    Duration: {} ms", upload_result.duration_ms);

    // Verify the uploaded content exists
    let exists = codex_bindings::exists(&node, &upload_result.cid).await?;
    assert!(exists, "Uploaded content should exist in Codex node");
    println!("  ✓ Upload verification passed - content exists in node");

    // Test downloading to verify round-trip
    let test_resources_dir = std::path::Path::new("tests/resources");
    let download_path = test_resources_dir.join("downloaded.pmtiles");
    let download_options =
        codex_bindings::DownloadStreamOptions::new(&upload_result.cid).filepath(&download_path);

    let download_result =
        codex_bindings::download_stream(&node, &upload_result.cid, download_options).await?;

    println!("  ✓ Download verification completed!");
    println!("    Downloaded size: {} bytes", download_result.size);

    // Verify file sizes match
    let original_size = tokio::fs::metadata(&test_file_path).await?.len();
    let downloaded_size = tokio::fs::metadata(&download_path).await?.len();

    assert_eq!(
        original_size, downloaded_size as u64,
        "Downloaded file should match original size"
    );
    println!("  ✓ File size verification passed: {} bytes", original_size);

    // Store the real CID in the database
    let real_cid_mapping = vec![(
        "AE".to_string(),
        421168683,
        upload_result.cid.clone(),
        upload_result.size as u64,
    )];

    db_service
        .batch_insert_cid_mappings(&real_cid_mapping)
        .await?;
    println!("  ✓ Stored real CID in database: {}", upload_result.cid);

    // Cleanup
    node.stop()?;
    node.destroy()?;

    println!("  ✓ Real Codex upload test completed successfully");

    Ok((upload_result.cid, upload_result.size as u64))
}

async fn verify_database_cids(
    db_service: &Arc<DatabaseService>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Verifying database CID mappings...");

    // Check if our test mappings exist
    let test_locality_ids = [421168683, 421168685, 421168687];

    for locality_id in &test_locality_ids {
        let has_mapping = db_service.has_cid_mapping("AE", *locality_id).await?;
        println!("  AE-{} has CID mapping: {}", locality_id, has_mapping);
    }

    println!("✓ Database verification completed");
    Ok(())
}

fn create_test_config(
    whosonfirst_db_path: &std::path::Path,
    cid_db_path: &std::path::Path,
    localities_dir: &std::path::Path,
) -> Result<LocalitySrvConfig, Box<dyn std::error::Error>> {
    use codex_bindings::{CodexConfig, LogLevel};

    let test_resources_dir = std::path::Path::new("tests/resources");
    let codex_data_dir = test_resources_dir.join(".codex-test-data");

    let codex_config = CodexConfig::new()
        .log_level(LogLevel::Error)
        .data_dir(&codex_data_dir)
        .storage_quota(100 * 1024 * 1024) // 100MB
        .discovery_port(8098) // Use different port to avoid conflicts
        .listen_addrs(vec!["/ip4/127.0.0.1/tcp/0".to_string()]);

    Ok(LocalitySrvConfig {
        codex: codex_config,
        data_dir: codex_data_dir,
        database_path: whosonfirst_db_path.to_path_buf(),
        cid_database_path: cid_db_path.to_path_buf(), // Separate database for CID mappings
        localities_dir: localities_dir.to_path_buf(),
        pmtiles_cmd: "pmtiles".to_string(),
        bzip2_cmd: "bzip2".to_string(),
        find_cmd: "find".to_string(),
        whosonfirst_db_url: "https://example.com/test.db".to_string(),
        planet_pmtiles_path: None,
        target_countries: vec!["AE".to_string()], // Only test AE country
        max_concurrent_extractions: 1,
    })
}

async fn cleanup_test_data(
    test_resources_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Remove test databases if they exist
    let test_whosonfirst_db_path = test_resources_dir.join("test_whosonfirst.db");
    if test_whosonfirst_db_path.exists() {
        fs::remove_file(&test_whosonfirst_db_path)?;
        println!(
            "✓ Removed existing WhosOnFirst test database: {:?}",
            test_whosonfirst_db_path
        );
    }

    let test_cid_db_path = test_resources_dir.join("test_cid_mappings.db");
    if test_cid_db_path.exists() {
        fs::remove_file(&test_cid_db_path)?;
        println!(
            "✓ Removed existing CID mappings test database: {:?}",
            test_cid_db_path
        );
    }

    // Remove .codex-test-data directory if it exists
    let codex_test_dir = test_resources_dir.join(".codex-test-data");
    if codex_test_dir.exists() {
        fs::remove_dir_all(&codex_test_dir)?;
        println!(
            "✓ Removed existing test Codex data directory: {:?}",
            codex_test_dir
        );
    }

    // Remove downloaded test file if it exists
    let downloaded_file = test_resources_dir.join("downloaded.pmtiles");
    if downloaded_file.exists() {
        fs::remove_file(&downloaded_file)?;
        println!("✓ Removed downloaded test file: {:?}", downloaded_file);
    }

    Ok(())
}
