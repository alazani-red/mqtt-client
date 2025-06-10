use serde::Deserialize;

use std::{fs, process};
// 設定ファイルの構造体を定義
#[derive(Debug, Deserialize)]
pub struct Config {
    pub scheme: Option<String>,
    pub broker_address: String,
    pub broker_port: u16,
    pub client_id: String,
    pub topics: Vec<String>,
    pub qos: Vec<i32>,
    pub clean_session: Option<bool>,
    pub username: Option<String>,
    pub password: Option<String>,
    // log_directory: Option<String>,
    // log_level: Option<String>,
    // CA証明書のパスを追加
    pub ca_cert_path: Option<String>,
    // クライアント証明書とキーのパス（相互認証が必要な場合）
    pub client_combined_path: Option<String>,
}

pub fn get_config() -> Config {
    // 設定ファイルを読み込む
    let config_file = "config.yaml";
    let config: Config = match fs::File::open(config_file) {
        Ok(file) => {
            match serde_yaml::from_reader(file) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("Error parsing config file '{}': {}", config_file, e);
                    process::exit(1);
                }
            }
        },
        Err(e) => {
            eprintln!("Error opening config file '{}': {}", config_file, e);
            process::exit(1);
        }
    };
    return config;
}
