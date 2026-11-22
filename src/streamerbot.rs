//! Lien avec Streamer.bot via son serveur WebSocket (Servers/Clients → WebSocket Server).
//!
//! - Réglages (adresse, port, chemin) lus dans `Documents\Streamer.bot\data\settings.json`,
//!   sauf si la section `[streamerbot]` de la config les impose.
//! - Authentification gérée si elle est activée dans Streamer.bot (mot de passe dans la config).
//! - Le démon ouvre une connexion le temps de lancer l'action (quelques ms en local) et
//!   vérifie la disponibilité de Streamer.bot par une simple connexion TCP toutes les 15 s.

use std::{
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::PathBuf,
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_websockets::{ClientBuilder, MaybeTlsStream, Message, WebSocketStream};

use crate::{app::AppEvent, config::StreamerbotConfig};

/// Réglages effectifs de connexion.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub endpoint: String,
    pub password: Option<String>,
}

impl Target {
    fn url(&self) -> String {
        let endpoint = if self.endpoint.starts_with('/') { self.endpoint.clone() } else { format!("/{}", self.endpoint) };
        format!("ws://{}:{}{}", self.host, self.port, endpoint)
    }
}

/// Réglages : config Soomfon d'abord, sinon ceux de Streamer.bot, sinon 127.0.0.1:8080/.
pub fn resolve(cfg: &StreamerbotConfig) -> Target {
    let path = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(r"Documents\Streamer.bot\data\settings.json");
    let ws: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| v["websockets"].clone())
        .unwrap_or(Value::Null);
    Target {
        host: cfg.host.clone().or_else(|| ws["address"].as_str().map(str::to_string)).unwrap_or_else(|| "127.0.0.1".into()),
        port: cfg.port.or_else(|| ws["port"].as_u64().map(|p| p as u16)).unwrap_or(8080),
        endpoint: ws["endpoint"].as_str().unwrap_or("/").to_string(),
        password: cfg.password.clone(),
    }
}

/// Le serveur WebSocket de Streamer.bot répond-il ? (simple connexion TCP, ~1 ms en local)
pub fn reachable(target: &Target) -> bool {
    let addrs: Vec<SocketAddr> = (target.host.as_str(), target.port).to_socket_addrs().map(|a| a.collect()).unwrap_or_default();
    addrs.iter().any(|a| TcpStream::connect_timeout(a, Duration::from_millis(250)).is_ok())
}

/// Réponse d'authentification attendue par Streamer.bot (même schéma qu'OBS) :
/// base64(sha256(base64(sha256(mot_de_passe + sel)) + défi)).
pub fn auth_response(password: &str, salt: &str, challenge: &str) -> String {
    let secret = B64.encode(Sha256::digest(format!("{password}{salt}").as_bytes()));
    B64.encode(Sha256::digest(format!("{secret}{challenge}").as_bytes()))
}

/// Une action Streamer.bot, telle que listée dans l'éditeur.
#[derive(Debug, Clone, PartialEq)]
pub struct SbAction {
    pub name: String,
    pub group: String,
    pub enabled: bool,
}

const STEP_TIMEOUT: Duration = Duration::from_secs(3);

pub struct Client {
    ws: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    next_id: u64,
}

impl Client {
    /// Connexion, accueil (« Hello ») et authentification si Streamer.bot la demande.
    pub async fn connect(target: &Target) -> Result<Self, String> {
        let builder = ClientBuilder::new().uri(&target.url()).map_err(|e| format!("adresse invalide : {e}"))?;
        let (ws, _) = tokio::time::timeout(STEP_TIMEOUT, builder.connect())
            .await
            .map_err(|_| "Streamer.bot ne répond pas".to_string())?
            .map_err(|_| "Streamer.bot fermé ou serveur WebSocket arrêté".to_string())?;
        let mut client = Client { ws, next_id: 1 };

        // Le premier message est « Hello » ; il contient un défi si l'authentification est activée.
        let hello = client.read_json(STEP_TIMEOUT).await?;
        if let Some(auth) = hello.get("authentication").filter(|a| a.is_object()) {
            let password = target
                .password
                .as_deref()
                .ok_or("Streamer.bot demande un mot de passe : ajoute-le dans la section [streamerbot] de la config")?;
            let salt = auth["salt"].as_str().unwrap_or("");
            let challenge = auth["challenge"].as_str().unwrap_or("");
            let reply = client
                .request(json!({ "request": "Authenticate", "authentication": auth_response(password, salt, challenge) }))
                .await?;
            if reply["status"] != "ok" {
                return Err("mot de passe Streamer.bot refusé".into());
            }
        }
        Ok(client)
    }

    async fn read_json(&mut self, timeout: Duration) -> Result<Value, String> {
        loop {
            let msg = tokio::time::timeout(timeout, self.ws.next())
                .await
                .map_err(|_| "Streamer.bot ne répond pas".to_string())?
                .ok_or("connexion Streamer.bot fermée")?
                .map_err(|e| format!("connexion Streamer.bot : {e}"))?;
            if let Some(text) = msg.as_text() {
                if let Ok(v) = serde_json::from_str::<Value>(text) {
                    return Ok(v);
                }
            } else if msg.is_close() {
                return Err("connexion Streamer.bot fermée".into());
            }
        }
    }

    /// Envoie une requête et attend sa réponse (repérée par son id ; les événements sont ignorés).
    async fn request(&mut self, mut req: Value) -> Result<Value, String> {
        let id = format!("soomfon-{}", self.next_id);
        self.next_id += 1;
        req["id"] = Value::String(id.clone());
        self.ws.send(Message::text(req.to_string())).await.map_err(|e| format!("envoi à Streamer.bot : {e}"))?;
        loop {
            let reply = self.read_json(STEP_TIMEOUT).await?;
            if reply["id"] == id.as_str() {
                return Ok(reply);
            }
        }
    }

    /// Liste des actions définies dans Streamer.bot (triées par groupe puis nom).
    pub async fn actions(&mut self) -> Result<Vec<SbAction>, String> {
        let reply = self.request(json!({ "request": "GetActions" })).await?;
        let mut list: Vec<SbAction> = reply["actions"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| SbAction {
                        name: x["name"].as_str().unwrap_or("").to_string(),
                        group: x["group"].as_str().unwrap_or("").to_string(),
                        enabled: x["enabled"].as_bool().unwrap_or(true),
                    })
                    .filter(|x| !x.name.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        list.sort_by(|a, b| a.group.cmp(&b.group).then_with(|| a.name.cmp(&b.name)));
        Ok(list)
    }

    /// Lance une action par son nom.
    pub async fn do_action(&mut self, name: &str) -> Result<(), String> {
        let reply = self.request(json!({ "request": "DoAction", "action": { "name": name }, "args": {} })).await?;
        match reply["status"].as_str() {
            Some("ok") => Ok(()),
            _ => Err(reply["error"].as_str().unwrap_or("action refusée par Streamer.bot").to_string()),
        }
    }

    pub async fn close(mut self) {
        let _ = self.ws.close().await;
    }
}

/// Tâche du démon : lance les actions demandées et signale si Streamer.bot est joignable.
pub fn spawn(cfg: StreamerbotConfig, events: UnboundedSender<AppEvent>) -> UnboundedSender<SbCommand> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(run(cfg, rx, events));
    tx
}

#[derive(Debug, Clone)]
pub enum SbCommand {
    /// Lance l'action portant ce nom.
    Run(String),
    /// Nouveaux réglages (config rechargée).
    Settings(StreamerbotConfig),
}

async fn run(cfg: StreamerbotConfig, mut rx: UnboundedReceiver<SbCommand>, events: UnboundedSender<AppEvent>) {
    let mut target = resolve(&cfg);
    let mut last: Option<bool> = None;
    let mut probe = tokio::time::interval(Duration::from_secs(15));
    probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = probe.tick() => {
                let t = target.clone();
                let ok = tokio::task::spawn_blocking(move || reachable(&t)).await.unwrap_or(false);
                report(ok, &mut last, &target, &events);
            }
            cmd = rx.recv() => match cmd {
                None => return,
                Some(SbCommand::Settings(c)) => {
                    target = resolve(&c);
                    probe.reset_immediately();
                }
                Some(SbCommand::Run(name)) => {
                    let result = async {
                        let mut client = Client::connect(&target).await?;
                        let r = client.do_action(&name).await;
                        client.close().await;
                        r
                    }
                    .await;
                    match result {
                        Ok(()) => {
                            log::info!("Streamer.bot : action « {name} » lancée");
                            report(true, &mut last, &target, &events);
                        }
                        Err(e) => {
                            log::warn!("Streamer.bot : action « {name} » échouée : {e}");
                            if !reachable(&target) {
                                report(false, &mut last, &target, &events);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Prévient l'application uniquement quand la disponibilité change.
fn report(ok: bool, last: &mut Option<bool>, target: &Target, events: &UnboundedSender<AppEvent>) {
    if *last != Some(ok) {
        *last = Some(ok);
        if ok {
            log::info!("Streamer.bot joignable ({}:{})", target.host, target.port);
        } else {
            log::info!("Streamer.bot injoignable");
        }
        let _ = events.send(AppEvent::Streamerbot(ok));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reponse_authentification() {
        // Valeur calculée indépendamment avec Node.js (crypto) :
        // base64(sha256(base64(sha256("mdp" + "sel")) + "defi"))
        assert_eq!(auth_response("mdp", "sel", "defi"), "zD0Yn8FtOsG/vSu7CeqeedUVyjLiDcgLoTVAH+/L5cs=");
        assert_ne!(auth_response("mdp", "sel", "defi"), auth_response("mdp", "sel", "autre"));
    }

    #[test]
    fn url_websocket() {
        let t = Target { host: "127.0.0.1".into(), port: 8080, endpoint: "/".into(), password: None };
        assert_eq!(t.url(), "ws://127.0.0.1:8080/");
        let t = Target { endpoint: "sb".into(), ..t };
        assert_eq!(t.url(), "ws://127.0.0.1:8080/sb");
    }
}
