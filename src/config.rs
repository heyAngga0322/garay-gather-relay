use config::{Config, Environment, File};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load configuration: {0}")]
    LoadError(#[from] config::ConfigError),
    #[error("Failed to generate self-signed certificate: {0}")]
    CertificateGeneration(#[from] rcgen::Error),
    #[error("Failed to load TLS certificate: {0}")]
    TlsLoadError(String),
    #[error("Invalid TURN configuration: {0}")]
    TurnConfigError(String),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_address")]
    pub bind_address: SocketAddr,
    #[serde(default = "default_tls_enabled")]
    pub tls_enabled: bool,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    #[serde(default)]
    pub turn: Option<TurnConfig>,
    #[serde(default = "default_max_peers_per_room")]
    pub max_peers_per_room: usize,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_bind_address() -> SocketAddr {
    "0.0.0.0:3000".parse().unwrap()
}

fn default_tls_enabled() -> bool {
    false
}

fn default_max_peers_per_room() -> usize {
    4
}

fn default_log_format() -> String {
    "json".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TlsConfig {
    #[serde(default)]
    pub cert_path: Option<PathBuf>,
    #[serde(default)]
    pub key_path: Option<PathBuf>,
    #[serde(default = "default_auto_generate_cert")]
    pub auto_generate: bool,
}

fn default_auto_generate_cert() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TurnConfig {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

impl ServerConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(File::with_name("config").required(false))
            .add_source(Environment::with_prefix("GARAY").separator("__"))
            .build()?;

        Ok(config.try_deserialize()?)
    }

    pub fn load_tls_config(&self) -> Result<Option<Arc<rustls::ServerConfig>>, ConfigError> {
        if !self.tls_enabled {
            return Ok(None);
        }

        let tls_config = self.tls.as_ref().ok_or_else(|| {
            ConfigError::TlsLoadError("TLS enabled but no TLS configuration provided".to_string())
        })?;

        let (certs, key) = if let (Some(cert_path), Some(key_path)) =
            (&tls_config.cert_path, &tls_config.key_path)
        {
            // Load from files
            let certs_vec = rustls_pemfile::certs(&mut std::io::BufReader::new(
                std::fs::File::open(cert_path).map_err(|e| {
                    ConfigError::TlsLoadError(format!("Failed to open cert file: {e}"))
                })?,
            ))
            .map_err(|e| ConfigError::TlsLoadError(format!("Failed to parse cert: {e}")))?;

            let keys = rustls_pemfile::pkcs8_private_keys(&mut std::io::BufReader::new(
                std::fs::File::open(key_path).map_err(|e| {
                    ConfigError::TlsLoadError(format!("Failed to open key file: {e}"))
                })?,
            ))
            .map_err(|e| ConfigError::TlsLoadError(format!("Failed to parse key: {e}")))?;

            let key_vec = keys
                .into_iter()
                .next()
                .ok_or_else(|| ConfigError::TlsLoadError("No private key found".to_string()))?;

            // Convert to CertificateDer and PrivateKeyDer
            let certs: Vec<CertificateDer<'static>> =
                certs_vec.into_iter().map(CertificateDer::from).collect();
            let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_vec));

            (certs, key)
        } else if tls_config.auto_generate {
            // Generate self-signed certificate for development
            let (certs, key) = generate_self_signed_cert()?;
            (certs, key)
        } else {
            return Err(ConfigError::TlsLoadError(
                "TLS enabled but no certificate paths provided and auto_generate is false"
                    .to_string(),
            ));
        };

        let mut server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| ConfigError::TlsLoadError(format!("Failed to build TLS config: {e}")))?;

        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(Some(Arc::new(server_config)))
    }

    pub fn turn_config_json(&self) -> Option<serde_json::Value> {
        self.turn.as_ref().map(|turn| {
            serde_json::json!({
                "iceServers": turn.urls.iter().map(|url| {
                    if url.starts_with("turn:") || url.starts_with("turns:") {
                        serde_json::json!({
                            "urls": url,
                            "username": turn.username,
                            "credential": turn.credential
                        })
                    } else {
                        serde_json::json!({ "urls": url })
                    }
                }).collect::<Vec<_>>()
            })
        })
    }
}

fn generate_self_signed_cert()
-> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), ConfigError> {
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params.subject_alt_names = vec![
        rcgen::SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
        rcgen::SanType::DnsName("localhost".try_into().unwrap()),
    ];
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::DigitalSignature,
        rcgen::KeyUsagePurpose::KeyEncipherment,
    ];
    params.is_ca = rcgen::IsCa::NoCa;

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));

    Ok((vec![cert_der], key_der))
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: default_bind_address(),
            tls_enabled: default_tls_enabled(),
            tls: None,
            turn: None,
            max_peers_per_room: default_max_peers_per_room(),
            log_format: default_log_format(),
            log_level: default_log_level(),
        }
    }
}
