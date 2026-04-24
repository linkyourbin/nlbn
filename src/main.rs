use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use nlbn::checkpoint::{append_checkpoint, load_checkpoint};
use nlbn::*;
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() {
    // Initialize logger with custom format to hide module paths
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .format(|buf, record| {
            use std::io::Write;
            writeln!(
                buf,
                "[{} {} nlbn] {}",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                record.level(),
                record.args()
            )
        })
        .init();

    // Parse CLI arguments
    let args = Cli::parse();

    // Set debug logging if requested
    if args.debug {
        log::set_max_level(log::LevelFilter::Debug);
    }

    // Run the conversion
    if let Err(e) = run(args).await {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

async fn run(args: Cli) -> error::Result<()> {
    // Validate arguments
    args.validate()?;

    // Get list of LCSC IDs to process
    let lcsc_ids = args.get_lcsc_ids()?;
    let is_batch = lcsc_ids.len() > 1;

    // Setup output directories
    let lib_manager = Arc::new(LibraryManager::from_cli(&args)?);
    lib_manager.create_directories()?;

    // Load checkpoint and filter already-completed IDs
    let checkpoint_path = args.output.join(".checkpoint");
    let completed_ids = load_checkpoint(&checkpoint_path);
    let lcsc_ids: Vec<String> = if is_batch && !completed_ids.is_empty() {
        let before = lcsc_ids.len();
        let filtered: Vec<String> = lcsc_ids
            .into_iter()
            .filter(|id| !completed_ids.contains(id))
            .collect();
        if before != filtered.len() {
            log::info!(
                "Resuming: skipping {} already completed components",
                before - filtered.len()
            );
        }
        filtered
    } else {
        lcsc_ids
    };

    let total_count = lcsc_ids.len();
    if total_count == 0 {
        println!("All components already completed.");
        return Ok(());
    }

    if is_batch {
        log::info!("Batch mode: processing {} components", total_count);
        if args.parallel > 1 {
            log::info!("Parallel downloads: {} threads", args.parallel);
        }
    }

    // Initialize API
    let api = Arc::new(EasyedaApi::new());

    // Track statistics
    let success_count = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    let failed_ids = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let checkpoint_path = Arc::new(checkpoint_path);
    let args = Arc::new(args);

    // Setup progress bar for batch mode
    let pb = if is_batch {
        let pb = ProgressBar::new(total_count as u64);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {per_sec} ETA: {eta}")
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏  "));
        pb
    } else {
        ProgressBar::hidden()
    };

    if is_batch && args.parallel > 1 {
        // Async parallel processing with semaphore
        let semaphore = Arc::new(Semaphore::new(args.parallel));
        let mut join_set = JoinSet::new();
        let pb = Arc::new(pb);

        for (_index, lcsc_id) in lcsc_ids.into_iter().enumerate() {
            let sem = semaphore.clone();
            let api = api.clone();
            let lib_manager = lib_manager.clone();
            let args = args.clone();
            let success_count = success_count.clone();
            let failed_count = failed_count.clone();
            let failed_ids = failed_ids.clone();
            let pb = pb.clone();
            let checkpoint_path = checkpoint_path.clone();

            join_set.spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");

                pb.set_message(format!("{}", lcsc_id));

                match process_component(&args, &api, &lib_manager, &lcsc_id).await {
                    Ok(_) => {
                        success_count.fetch_add(1, Ordering::Relaxed);
                        append_checkpoint(&checkpoint_path, &lcsc_id);
                        pb.println(format!("✓ {}", lcsc_id));
                    }
                    Err(e) => {
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        failed_ids.lock().await.push(lcsc_id.clone());
                        pb.println(format!("✗ {} - {}", lcsc_id, e));
                        log::error!("Failed to process {}: {}", lcsc_id, e);
                    }
                }
                pb.inc(1);
            });
        }

        while let Some(result) = join_set.join_next().await {
            if let Err(e) = result {
                log::error!("Task panicked: {}", e);
            }
        }
        pb.finish_and_clear();
    } else {
        // Sequential processing mode
        for (_index, lcsc_id) in lcsc_ids.iter().enumerate() {
            if is_batch {
                pb.set_message(format!("{}", lcsc_id));
            } else {
                log::info!("Starting conversion for LCSC ID: {}", lcsc_id);
            }

            match process_component(&args, &api, &lib_manager, lcsc_id).await {
                Ok(_) => {
                    success_count.fetch_add(1, Ordering::Relaxed);
                    append_checkpoint(&checkpoint_path, lcsc_id);
                    if is_batch {
                        pb.println(format!("✓ {}", lcsc_id));
                        pb.inc(1);
                    }
                }
                Err(e) => {
                    failed_count.fetch_add(1, Ordering::Relaxed);
                    failed_ids.lock().await.push(lcsc_id.clone());

                    if args.continue_on_error {
                        if is_batch {
                            pb.println(format!("✗ {} - {}", lcsc_id, e));
                            pb.inc(1);
                        } else {
                            eprintln!("✗ Failed: {} - {}", lcsc_id, e);
                        }
                        log::error!("Failed to process {}: {}", lcsc_id, e);
                    } else {
                        pb.finish_and_clear();
                        return Err(e);
                    }
                }
            }
        }
        pb.finish_and_clear();
    }

    let success = success_count.load(Ordering::Relaxed);
    let failed = failed_count.load(Ordering::Relaxed);
    let failed_list = failed_ids.lock().await.clone();

    // Print summary for batch mode
    if is_batch {
        println!("\n{}", "=".repeat(60));
        println!("Batch conversion complete!");
        println!(
            "Total: {} | Success: {} | Failed: {}",
            total_count, success, failed
        );

        if !failed_list.is_empty() {
            println!("\nFailed components:");
            for id in &failed_list {
                println!("  - {}", id);
            }
        }

        println!("Output directory: {}", args.output.display());
        println!("{}", "=".repeat(60));
    } else {
        println!("\n✓ Conversion complete!");
        println!("Output directory: {}", args.output.display());
    }

    Ok(())
}

async fn process_component(
    args: &Cli,
    api: &EasyedaApi,
    lib_manager: &LibraryManager,
    lcsc_id: &str,
) -> error::Result<()> {
    // Fetch component data from EasyEDA API
    let component_data = api.get_component_data(lcsc_id).await?;

    log::info!("Fetched component: {}", component_data.title);

    // Process symbol (if requested)
    if args.symbol || args.full {
        log::info!("Converting symbol...");
        symbol_converter::convert_symbol(args, &component_data, lib_manager, lcsc_id)?;
    }

    // Process footprint (if requested)
    if args.footprint || args.full {
        log::info!("Converting footprint...");
        footprint_converter::convert_footprint(args, &component_data, lib_manager, lcsc_id)?;
    }

    // Process 3D model (if requested)
    if args.model_3d || args.full {
        model_converter::convert_3d_model(args, api, &component_data, lib_manager, lcsc_id).await?;
    }

    Ok(())
}
