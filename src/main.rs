use anyhow::{Context, Result};
use reqwest::{get, Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::fs::read_to_string;
use std::io::{stdout, BufWriter, Write};
use std::time::{Duration, Instant};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::sleep;

#[forbid(unsafe_code)]

const CONFIG_PATH: &'static str = "./config.json";

#[tokio::main]
async fn main() -> Result<()> {
    if let Ok(_) = File::open(CONFIG_PATH).await {
        let client = Client::new();
        let config: Config = serde_json::from_str(
            &read_to_string(CONFIG_PATH).context("Could not read config file")?,
        )
        .context("Could not deserialize config file")?;
        let update_time: f32 = 1.0 / config.updates_per_second as f32;

        let mut request_number: usize = 0;
        let mut w = BufWriter::new(stdout().lock());
        let froggi_url = config.froggi_url + "/ocr";

        loop {
            let start_time = Instant::now();
            request_number += 1;

            writeln!(w, "\n({})", request_number).context("Could not write to BufWriter")?;

            match get(&config.ocr_url).await {
                Ok(b0) => {
                    writeln!(
                        w,
                        "{} from {}\nSending OCR data to {}",
                        b0.status(),
                        &config.ocr_url,
                        &froggi_url
                    )
                    .context("Could not write to BufWriter")?;

                    match client
                        .post(&froggi_url)
                        .body(
                            b0.text()
                                .await
                                .context("Could not cast ocr response body")?,
                        )
                        .header("api-key", &config.api_key)
                        .send()
                        .await
                    {
                        Ok(b1) => {
                            writeln!(w, "{} from {}", b1.status(), &froggi_url)
                                .context("Could not write to BufWriter")?;
                        }
                        Err(e) => {
                            writeln!(w, "{e}").context("Could not write to BufWriter")?;
                        }
                    }
                }
                Err(e) => {
                    writeln!(w, "{e}").context("Could not write to BufWriter")?;
                }
            }

            w.flush().context("Could not flush BufWriter to stdout")?;

            let delta_time = (start_time - Instant::now()).as_secs_f32();
            if delta_time < update_time {
                sleep(Duration::from_secs_f32(update_time - delta_time)).await;
            }
        }
    } else {
        // TO DO: make an intricate config process
        println!("It looks like froggi-ocr hasn't been set up yet, starting config process now...\nEnsure froggi is LAN or WAN accessible and type in its URL\n");

        let client = Client::new();

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);

        let froggi_url = loop {
            let mut froggi_url = String::new();            

            reader.read_line(&mut froggi_url).await.context("Failed to read line from stdin")?;
            froggi_url = froggi_url.trim().to_string();

            if froggi_url.starts_with("https://") {
                println!("Testing connection with froggi...");
                if let Ok(r) = client.head(&froggi_url).timeout(Duration::from_secs(10)).send().await {
                    if r.status() == StatusCode::OK {
                        println!("Connection with froggi successful! Adding to config...\n");
                        break froggi_url;
                    } else {
                        println!("Connection with froggi unsuccessful, try again\n");
                        continue;
                    }
                }
            } else if froggi_url.starts_with("http://") {
                let mut a = String::new();
                let stdin = tokio::io::stdin();
                let mut reader = BufReader::new(stdin);

                println!("It looks like the  url uses http (unencrypted) instead of https (encrypted). Sending API keys over http is discouraged and a bad security practice. Unless this is 100% intentional, https should be used.\nSwitch to https? (Y or n)\n");

                reader.read_line(&mut a).await.context("Failed to read line from stdin")?;

                if a.trim() == "n" {
                    println!("Using http anyway\nTesting connection with froggi...");
                    if let Ok(r) = client.head(&froggi_url).timeout(Duration::from_secs(10)).send().await {
                        if r.status() == StatusCode::OK {
                            println!("Connection with froggi successful! Adding to config...\n");
                            break froggi_url;
                        } else {
                            println!("Connection with froggi unsuccessful, try again\n");
                            continue;
                        }
                    }
                } else {
                    println!("Using https");
                    let mut url_vec = froggi_url.split("://").collect::<Vec<&str>>();

                    url_vec[0] = "https";
                    froggi_url = url_vec.join("://");

                    println!("Testing connection with froggi...");
                    if let Ok(r) = client.head(&froggi_url).timeout(Duration::from_secs(10)).send().await {
                        if r.status() == StatusCode::OK {
                            println!("Connection with froggi successful! Adding to config...\n");
                            break froggi_url;
                        } else {
                            println!("Connection with froggi unsuccessful, try again\n");
                            continue;
                        }
                    }
                }
            } else {
                froggi_url = format!("https://{}", froggi_url);

                println!("Using https ({})\nTesting connection with froggi...", froggi_url);

                if let Ok(r) = client.head(&froggi_url).timeout(Duration::from_secs(10)).send().await {
                    if r.status() == StatusCode::OK {
                        println!("Connection with froggi successful! Adding to config...\n");
                        break froggi_url;
                    } else {
                        println!("Connection with froggi unsuccessful, try again\n");
                        continue;
                    }
                }
            }
        };

        println!("What is the API key for froggi?\n");
        let api_key = loop {
            let mut key = String::new();

            reader.read_line(&mut key).await.context("Failed to read line from stdin")?;
            key = key.trim().to_string();

            println!("Testing API key...");
            
            let r = client.post(format!("{}/api/key/check/{}", froggi_url, key)).send().await.context("Could not check api key with froggi")?;

            if r.status() == StatusCode::OK {
                println!("API key valid! Adding to config...\n");
                break key;
            } else {
                println!("API key invalid, try again\n");
                continue;
            }
        };

        let config = Config {
            froggi_url: froggi_url,
            api_key: api_key,
            ..Config::default()
        };

        println!("Writing config to config.json...");

        let mut f = File::create("./config.json").await.context("Could not create config.json")?;

        f.write_all(serde_json::to_string_pretty(&config).context("Could not serialize config")?.as_bytes()).await.context("Could not write to config.json")?;

        println!("Config written successfully!");
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Config {
    api_key: String,
    ocr_url: String,
    froggi_url: String,
    updates_per_second: u8,
}

impl Config {
    fn default() -> Config {
        Config {
            api_key: String::new(),
            ocr_url: String::from("http://localhost:18099/json?pivot"),
            froggi_url: String::new(),
            updates_per_second: 5,
        }
    }
}
