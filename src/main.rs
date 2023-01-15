use config::{Config, ServiceConfig};
use log::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv6Addr},
    str::FromStr, fmt::Display,
};

mod config;

#[derive(Deserialize, Debug)]
struct IpInfo {
    ip: Ipv6Addr,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct GandiError {
    object: String,
    cause: String,
    message: String,
    code: u32,
}

impl Display for GandiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("[{}][{}] {}", self.code, self.object, self.message))
    }
}

#[derive(Serialize, Debug)]
struct GandiRecordRequest {
    rrset_values: Vec<String>,
    rrset_ttl: u32,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct GandiRecordResponse {
    rrset_values: Vec<String>,
    rrset_ttl: u32,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct GandiMessage {
    message: String,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum GandiResponse {
    Error(GandiError),
    GandiRecordResponse(GandiRecordResponse),
    Message(GandiMessage),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup
    env_logger::init();
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/etc/dynsix/config.toml".to_string());
    let config = Config::load(config_path)?;

    let client = Client::builder()
        .local_address(IpAddr::from_str("::0").ok())
        .build()?;

    // Resolve the public ip
    let ip_info = get_ip(&client, &config.query_server)
        .await
        .expect("Failed to get public IP");
    debug!("Got public ip: {}", ip_info.ip);

    for (name, service) in config.services {
        let service_ip = merge_ips(ip_info.ip, service.suffix);
        debug!(
            target: &format!("service-{name}"),
            "Merged IP: {service_ip}"
        );

        match get_gandi_ip(&client, &config.token, &service.fqdn, &service.name).await? {
            GandiResponse::Error(GandiError { code: 404, .. }) => {
                debug!(
                    target: &format!("service-{name}"),
                    "No AAAA record found for {}.{}", service.fqdn, service.name
                );
                match set_gandi_record(&client, &config.token, &service, &service_ip).await? {
                    GandiResponse::Error(e) => error!(
                        target: &format!("service-{name}"),
                        "Ran into an error while setting record: {e:?}"
                    ),
                    GandiResponse::Message(record) => info!(
                        target: &format!("service-{name}"),
                        "Successfully set AAAA record: {record:?}"
                    ),
                    _ => {}
                }
            }
            GandiResponse::Error(e) => println!("{e:?}"),
            GandiResponse::GandiRecordResponse(record) => {
                info!(
                    target: &format!("service-{name}"),
                    "Found an existing AAAA record for {}.{}: {:?}",
                    service.name,
                    service.fqdn,
                    record.rrset_values
                );
                if !Ipv6Addr::from_str(&record.rrset_values[0])
                    .unwrap()
                    .eq(&service_ip)
                {
                    debug!(target: &format!("service-{name}"), "Record differs");
                    match update_gandi_record(&client, &config.token, &service, &service_ip).await?
                    {
                        GandiResponse::Error(e) => error!(
                            target: &format!("service-{name}"),
                            "Ran into an error while setting record: {e:?}"
                        ),
                        GandiResponse::Message(record) => info!(
                            target: &format!("service-{name}"),
                            "Successfully updated AAAA record: {record:?}"
                        ),
                        _ => {}
                    }
                } else {
                    info!(
                        target: &format!("service-{name}"),
                        "Record was already set to the correct address"
                    );
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn merge_ips(prefix: Ipv6Addr, suffix: Ipv6Addr) -> Ipv6Addr {
    let prefix_segments = prefix.segments();
    let suffix_segments = suffix.segments();

    Ipv6Addr::new(
        prefix_segments[0],
        prefix_segments[1],
        prefix_segments[2],
        prefix_segments[3],
        suffix_segments[4],
        suffix_segments[5],
        suffix_segments[6],
        suffix_segments[7],
    )
}

async fn set_gandi_record(
    client: &Client,
    token: &str,
    service: &ServiceConfig,
    ip: &Ipv6Addr,
) -> Result<GandiResponse, reqwest::Error> {
    debug!("Fetching public ip");
    client
        .post(format!(
            "https://api.gandi.net/v5/livedns/domains/{}/records/{}/AAAA",
            service.fqdn, service.name
        ))
        .header("Accept", "application/json")
        .header("Authorization", format!("ApiKey {}", token))
        .json(&GandiRecordRequest {
            rrset_values: vec![ip.to_string()],
            rrset_ttl: service.ttl,
        })
        .send()
        .await?
        .json()
        .await
}

async fn update_gandi_record(
    client: &Client,
    token: &str,
    service: &ServiceConfig,
    ip: &Ipv6Addr,
) -> Result<GandiResponse, reqwest::Error> {
    client
        .put(format!(
            "https://api.gandi.net/v5/livedns/domains/{}/records/{}/AAAA",
            service.fqdn, service.name
        ))
        .header("Accept", "application/json")
        .header("Authorization", format!("ApiKey {}", token))
        .json(&GandiRecordRequest {
            rrset_values: vec![ip.to_string()],
            rrset_ttl: service.ttl,
        })
        .send()
        .await?
        .json()
        .await
}

async fn get_gandi_ip(
    client: &Client,
    token: &str,
    fqdn: &str,
    name: &str,
) -> Result<GandiResponse, reqwest::Error> {
    client
        .get(format!(
            "https://api.gandi.net/v5/livedns/domains/{}/records/{}/AAAA",
            fqdn, name
        ))
        .header("Accept", "application/json")
        .header("Authorization", format!("ApiKey {}", token))
        .send()
        .await?
        .json()
        .await
}

async fn get_ip(client: &Client, ip_query_server: &str) -> Result<IpInfo, reqwest::Error> {
    client
        .get(ip_query_server)
        .header("Accept", "application/json")
        .send()
        .await?
        .json::<IpInfo>()
        .await
}
