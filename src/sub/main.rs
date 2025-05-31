use std::{
    fs, io::Seek, process, sync::Arc, time::Duration
};
use rumqttc::{tokio_rustls::rustls::{ClientConfig, RootCertStore}, Client, Event, MqttOptions, Packet, QoS, Transport};
use serde::Deserialize;
use tokio::time; 

// TODO: ログ出力機能、ログ出力設定を追加する

// 設定ファイルの構造体を定義
#[derive(Debug, Deserialize)]
struct Config {
    scheme: Option<String>,
    broker_address: String,
    broker_port: u16,
    client_id: String,
    topics: Vec<String>,
    qos: Vec<i32>,
    clean_session: Option<bool>,
    username: Option<String>,
    password: Option<String>,
    // log_directory: Option<String>,
    // log_level: Option<String>,
    // CA証明書のパスを追加
    ca_cert_path: Option<String>,
    // クライアント証明書とキーのパス（相互認証が必要な場合）
    client_combined_path: Option<String>,
}

// 複数のトピックを購読する
async fn subscribe_topics(cli: &mut Client, topics: &[String], qos_values: &[QoS]) {
    for (i, topic) in topics.iter().enumerate() {
        // QoS が指定されていない場合は QoS::AtMostOnce (QoS 0) をデフォルトとする
        let qos = qos_values.get(i).copied().unwrap_or(QoS::AtMostOnce);
        if let Err(e) = cli.subscribe(topic, qos) {
            eprintln!("トピック '{}' (QoS {:?}) の購読中にエラーが発生しました: {:?}", topic, qos, e);
            process::exit(1);
        }
        println!("トピック: '{}' (QoS {:?}) を購読しました。", topic, qos);
    }
}

#[tokio::main]
async fn main() {
    // 設定ファイルを読み込む
    let config_file = "config.yaml";
    let config: Config = match fs::File::open(config_file) {
        Ok(file) => {
            match serde_yaml::from_reader(file) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("設定ファイル '{}' のパース中にエラーが発生しました: {}", config_file, e);
                    process::exit(1);
                }
            }
        },
        Err(e) => {
            eprintln!("設定ファイル '{}' のオープン中にエラーが発生しました: {}", config_file, e);
            process::exit(1);
        }
    };

    let mut mqtt_options = MqttOptions::new(config.client_id, config.broker_address, config.broker_port);
    mqtt_options.set_keep_alive(Duration::from_secs(20));
    mqtt_options.set_clean_session(config.clean_session.unwrap_or(true));

    // ユーザー名とパスワードが指定されていれば設定
    if let Some(username) = &config.username {
        let password = config.password.as_deref().unwrap_or("");
        mqtt_options.set_credentials(username, password);
    }

    // SSL/TLS 設定
    if config.scheme.as_deref() == Some("ssl") || config.scheme.as_deref() == Some("mqtts") {
        let mut root_store = RootCertStore::empty();

        // CA証明書の読み込みと追加
        if let Some(ca_cert_path) = &config.ca_cert_path {
            let ca_cert_pem = fs::read(ca_cert_path).unwrap_or_else(|e| {
                eprintln!("CA証明書 '{}' の読み込み中にエラーが発生しました: {}", ca_cert_path, e);
                process::exit(1);
            });
            let mut ca_certs_reader = std::io::BufReader::new(std::io::Cursor::new(ca_cert_pem));
            let certs = rustls_pemfile::certs(&mut ca_certs_reader)
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            for cert in certs {
                root_store.add(cert).unwrap_or_else(|e| { 
                    eprintln!("CA証明書の追加中にエラーが発生しました: {}", e);
                    process::exit(1);
                });
            }
        } else {
            eprintln!("警告: SSL/TLS 接続用に CA 証明書のパスが指定されていません。");
        }

        // クライアント認証の準備
        let client_config = if let Some(client_combined_path) = &config.client_combined_path {
            let cert_key_pem = fs::read(client_combined_path).unwrap_or_else(|e| {
                eprintln!("クライアント証明書/キーファイル '{}' の読み込み中にエラーが発生しました: {}", client_combined_path, e);
                process::exit(1);
            });

            let mut reader = std::io::BufReader::new(std::io::Cursor::new(cert_key_pem));
            let certs = rustls_pemfile::certs(&mut reader)
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            reader.rewind().unwrap();

            let client_key_pkcs8 = rustls_pemfile::pkcs8_private_keys(&mut reader)
                .filter_map(Result::ok)
                .next()
                .unwrap_or_else(|| {
                    eprintln!("クライアントの秘密鍵が見つかりません。");
                    process::exit(1);
                });
            
            let client_key = rustls_pki_types::PrivateKeyDer::Pkcs8(client_key_pkcs8.into());

            // ClientConfig の構築
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_client_auth_cert(certs, client_key)
                .unwrap_or_else(|e| {
                    eprintln!("クライアント認証の設定に失敗しました: {}", e);
                    process::exit(1);
                })
        } else {
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        let tls_config = Arc::new(client_config);
        mqtt_options.set_transport(Transport::Tls(rumqttc::TlsConfiguration::Rustls(tls_config)));
    }

    let (mut client, mut eventloop) = Client::new(mqtt_options, 10); // 10 はイベントループのチャネル容量

    // 設定された QoS 値を rumqttc::QoS 型に変換
    let actual_qos: Vec<QoS> = if config.qos.len() < config.topics.len() && !config.qos.is_empty() {
        let default_qos_val = config.qos[0];
        let default_qos = match default_qos_val {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            2 => QoS::ExactlyOnce,
            _ => {
                eprintln!("設定ファイル内の不正な QoS 値: {}", default_qos_val);
                process::exit(1);
            }
        };
        vec![default_qos; config.topics.len()]
    } else if config.qos.is_empty() && !config.topics.is_empty() {
        vec![QoS::AtMostOnce; config.topics.len()] // デフォルトで QoS 0 を適用
    } else {
        config.qos.iter().map(|&q| {
            match q {
                0 => QoS::AtMostOnce,
                1 => QoS::AtLeastOnce,
                2 => QoS::ExactlyOnce,
                _ => {
                    eprintln!("設定ファイル内の不正な QoS 値: {}", q);
                    process::exit(1);
                }
            }
        }).collect()
    };

    // トピックの購読
    subscribe_topics(&mut client, &config.topics, &actual_qos).await;

    println!("MQTT イベントを処理中...");
    loop {
        match eventloop.eventloop.poll().await {
            Ok(event) => {
                // println!("受信イベント: {:?}", event); // 詳細なイベントログが必要な場合にコメントを外す
                if let Event::Incoming(Packet::Publish(p)) = event {
                    println!("トピック: {}", p.topic);
                    println!("ペイロード: {}", String::from_utf8_lossy(&p.payload));
                    println!("QoS: {:?}", p.qos);
                } else if let Event::Incoming(Packet::ConnAck(_)) = event {
                    println!("ブローカーに接続しました。");
                } else if let Event::Outgoing(rumqttc::Outgoing::Disconnect) = event {
                    println!("ブローカーから切断しました。");
                    break;  // イベントループを終了
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("disconnected") {
                    eprintln!("ブローカーへの接続が閉じられました。再接続を試行中...");
                    time::sleep(Duration::from_secs(5)).await;
                } else {
                    eprintln!("イベントループでエラーが発生しました: {:?}", e);
                    time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    println!("終了します。");
}