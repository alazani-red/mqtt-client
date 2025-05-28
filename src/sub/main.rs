use std::{
    env,
    process,
    thread,
    time::Duration,
    fs,
};

extern crate paho_mqtt as mqtt;
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
    username: String,
    password: String,
}

// Reconnect to the broker when connection is lost.
fn try_reconnect(cli: &mqtt::Client) -> bool
{
    println!("Connection lost. Waiting to retry connection");
    for _ in 0..12 {
        thread::sleep(Duration::from_millis(5000));
        if cli.reconnect().is_ok() {
            println!("Successfully reconnected");
            return true;
        }
    }
    println!("Unable to reconnect after several attempts.");
    false
}

// Subscribes to multiple topics.
fn subscribe_topics(cli: &mqtt::Client, topics: &[String], qos: &[i32]) {
    let topic_refs: Vec<&str> = topics.iter().map(|s| s.as_str()).collect();
    if let Err(e) = cli.subscribe_many(&topic_refs, qos) {
        println!("Error subscribes topics: {:?}", e);
        process::exit(1);
    }
}

fn main() {
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

    // main関数内で"tcp://"を結合
    // env::args().nth(1)が優先されるように、少しロジックを調整します。
    let default_broker_uri = format!("tcp://{}:{}", config.broker_address, config.broker_port); // ここで結合
    let host = env::args().nth(1).unwrap_or_else(||
        default_broker_uri
    );

    // Define the set of options for the create.
    // Use an ID for a persistent session.
    let create_opts = mqtt::CreateOptionsBuilder::new()
        .server_uri(host)
        .client_id(config.client_id.clone())
        .finalize();

    // Create a client.
    let cli = mqtt::Client::new(create_opts).unwrap_or_else(|err| {
        println!("Error creating the client: {:?}", err);
        process::exit(1);
    });

    // Initialize the consumer before connecting.
    let rx = cli.start_consuming();

    // Define the set of options for the connection.
    let lwt = mqtt::MessageBuilder::new()
        .topic("test")
        .payload("Consumer lost connection")
        .finalize();
    let conn_opts = mqtt::ConnectOptionsBuilder::new()
        .keep_alive_interval(Duration::from_secs(20))
        .clean_session(true)
        .will_message(lwt)
        .user_name(config.username.clone())
        .password(config.password.clone())
        .finalize();

    // Connect and wait for it to complete or fail.
    if let Err(e) = cli.connect(conn_opts) {
        println!("Unable to connect:\n\t{:?}", e);
        process::exit(1);
    }

    let actual_qos: Vec<i32> = if config.qos.len() < config.topics.len() && !config.qos.is_empty() {
        // QoSがトピック数より少なく、かつqosが空でない場合、最初のQoSを全てのトピックに適用
        let default_qos = config.qos[0]; // 最初の要素を取得
        vec![default_qos; config.topics.len()] // トピックの数だけ繰り返す
    } else if config.qos.is_empty() && !config.topics.is_empty() {
        // QoSが空で、トピックがある場合、デフォルトQoS(0)を適用
        vec![0; config.topics.len()]
    }
    else {
        // QoSがトピック数以上ある場合、またはQoSが空だがトピックも空の場合、qosをそのまま使用
        config.qos.clone()
    };    
    
    subscribe_topics(&cli, &config.topics, &actual_qos);

    println!("Processing requests...");
    for msg in rx.iter() {
        if let Some(msg) = msg {
            println!("{}", msg);
        }
        else if !cli.is_connected() {
            if try_reconnect(&cli) {
                println!("Resubscribe topics...");
                subscribe_topics(&cli, &config.topics, &config.qos);
            } else {
                break;
            }
        }
    }

    // If still connected, then disconnect now.
    if cli.is_connected() {
        println!("Disconnecting");
        let topic_refs: Vec<&str> = config.topics.iter().map(|s| s.as_str()).collect();
        cli.unsubscribe_many(&topic_refs).unwrap();
        cli.disconnect(None).unwrap();
    }
    println!("Exiting");
}