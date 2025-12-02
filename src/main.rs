mod log;
mod macros;
mod project;
mod token;
use std::{collections::HashMap, path::PathBuf};

use crate::macros::ProjectGlobalMacros;
use project::Project;
use serde_json::Value;

fn main() {
    // 入口文件路径（从命令行参数获取）
    let entry = std::env::args()
        .nth(1)
        .expect("请提供入口文件路径作为第一个参数");
    let export_path = std::env::args()
        .nth(2)
        .expect("请提供导出路径作为第二个参数（必须为文件夹）");
    let export_path = PathBuf::from(export_path);

    // 尝试解析当前目录的 dlua.json
    let _config_path = std::env::current_dir()
        .expect("获取当前目录失败")
        .join("dlua.json");

    // 加载配置文件内容（如果存在）
    let _config: Option<Value> = if _config_path.exists() {
        let config_content = std::fs::read_to_string(&_config_path).expect("读取配置文件失败");
        let json: Value = serde_json::from_str(&config_content).expect("解析配置文件失败");
        Some(json)
    } else {
        None
    };

    let require_paths: Option<Vec<String>> = if let Some(config) = &_config {
        if let Some(paths) = config.get("require_paths") {
            if let Some(arr) = paths.as_array() {
                Some(
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect(),
                )
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // 记录当前时间
    let _start_time = std::time::Instant::now();
    let proj = Project::load(&entry, require_paths, &export_path).expect("加载项目失败");
    let _duration = _start_time.elapsed();
    println!("项目加载完成，耗时: {:?}", _duration);

    // 再记录时间
    let _start_time = std::time::Instant::now();
    let file_global_macros = ProjectGlobalMacros::new(&proj);
    let _duration = _start_time.elapsed();
    println!("全局宏收集完成，耗时: {:?}", _duration);

    let level_map: HashMap<String, u8> = HashMap::from([
        ("debug".to_string(), 0),
        ("info".to_string(), 1),
        ("release".to_string(), 2),
    ]);

    let user_level = 1;

    let _start_time = std::time::Instant::now();
    file_global_macros.expand_all_with_levels(&proj, user_level, &level_map, &export_path);
    let _duration = _start_time.elapsed();
    println!("宏展开完成，耗时: {:?}", _duration);
    println!("{:?}", file_global_macros.macro_map);
}
