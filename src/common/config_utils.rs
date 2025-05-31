
use serde::Deserialize;

// 設定ファイルの構造体を定義
#[derive(Debug, Deserialize)]
struct Config {
    // brokerフィールドはプロトコル部分（tcp://）を含まない形にする
    broker_address: String, 
    broker_port: u16,
    client_id: String,
    topics: Vec<String>,
    qos: Vec<i32>,
    // qos: i32,
    username: Option<String>,
    password: Option<String>,
    log_directory: Option<String>,
    log_level: Option<String>, 
}

fn get_config() -> Config {
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
    config
}
