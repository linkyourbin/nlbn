use crate::cli::Cli;
use crate::converter::sanitize_name;
use crate::easyeda::{ComponentData, EasyedaApi};
use crate::error::Result;
use crate::kicad::ModelExporter;
use crate::library::{FileWriteStatus, LibraryManager};

pub async fn convert_3d_model(
    args: &Cli,
    api: &EasyedaApi,
    component_data: &ComponentData,
    lib_manager: &LibraryManager,
    lcsc_id: &str,
) -> Result<()> {
    if let Some(model_info) = &component_data.model_3d {
        log::info!("Converting 3D model...");

        // Use LCSC ID as unique identifier to prevent name collisions
        let model_name = format!("{}_{}", sanitize_name(&model_info.title), lcsc_id);
        let exporter = ModelExporter::new();

        let wrl_path = lib_manager.get_wrl_path(&model_name);
        let step_path = lib_manager.get_step_path(&model_name);
        let need_wrl = args.overwrite || !wrl_path.exists();
        let need_step = args.overwrite || !step_path.exists();

        if !need_wrl && !need_step {
            println!("Skipped existing 3D model: {}", model_name);
            return Ok(());
        }

        let obj_future = async {
            if need_wrl {
                Some(api.download_3d_obj(&model_info.uuid).await)
            } else {
                None
            }
        };
        let step_future = async {
            if need_step {
                Some(
                    api.download_3d_to_file(&model_info.uuid, "STEP", &step_path)
                        .await,
                )
            } else {
                None
            }
        };

        let (obj_result, step_result) = tokio::join!(obj_future, step_future);

        let mut has_wrl = !need_wrl;
        let mut has_step = !need_step;

        if let Some(obj_result) = obj_result {
            match obj_result {
                Ok(obj_data) => match exporter.obj_to_wrl(&obj_data) {
                    Ok(wrl_data) => {
                        match lib_manager.write_wrl_model_if_needed(
                            &model_name,
                            &wrl_data,
                            args.overwrite,
                        ) {
                            Ok((_, FileWriteStatus::Written)) => {
                                log::info!("\u{2713} WRL model converted: {}", model_name);
                                has_wrl = true;
                            }
                            Ok((_, FileWriteStatus::Skipped)) => {
                                has_wrl = true;
                            }
                            Err(e) => log::warn!("Failed to write WRL model: {}", e),
                        }
                    }
                    Err(e) => log::warn!("Failed to convert OBJ to WRL: {}", e),
                },
                Err(e) => log::warn!("Failed to download OBJ model: {}", e),
            }
        }

        if let Some(step_result) = step_result {
            match step_result {
                Ok(_) => {
                    log::info!("\u{2713} STEP model converted: {}", model_name);
                    has_step = true;
                }
                Err(e) => log::warn!("Failed to download STEP model: {}", e),
            }
        }

        match (has_wrl, has_step) {
            (true, true) => println!("\u{2713} 3D model converted: {} (WRL + STEP)", model_name),
            (true, false) => println!("\u{2713} 3D model converted: {} (WRL only)", model_name),
            (false, true) => println!("\u{2713} 3D model converted: {} (STEP only)", model_name),
            (false, false) => println!("\u{26a0} 3D model not available"),
        }
    } else {
        log::warn!("No 3D model metadata available for this component");
    }

    Ok(())
}
