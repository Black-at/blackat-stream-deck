//! Lanceur Blackat : un clic sur « Lancer » démarre, dans l'ordre,
//!   1. OBS Studio      → attend que son serveur WebSocket réponde (port lu dans OBS) ;
//!   2. Streamer.bot    → attend que son serveur WebSocket réponde (port lu dans Streamer.bot) ;
//!   3. le Stream Deck  → démon soomfon-deck, lancé sans fenêtre.
//! Les applis déjà ouvertes ne sont pas relancées. Quand tout est prêt, la fenêtre se ferme.

#![windows_subsystem = "windows"] // pas de fenêtre console

use std::{
    net::{SocketAddr, TcpStream},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use slint::{ComponentHandle, ModelRc, PhysicalPosition, Timer, VecModel};

slint::include_modules!();

mod pack;

/// Progression écrite dans un fichier journal (mode ligne de commande, sans fenêtre).
struct LogProgress(std::sync::Mutex<std::fs::File>, &'static [&'static str]);

impl pack::backup::Progress for LogProgress {
    fn step(&self, index: usize, state: i32, detail: &str) {
        use std::io::Write;
        let tag = match state {
            1 => "…",
            2 => "OK",
            _ => "!!",
        };
        let _ = writeln!(self.0.lock().unwrap(), "[{tag}] {} : {detail}", self.1[index]);
    }
}

/// `blackat-launcher.exe --backup <dossier> --password <mdp> [--force]` : crée un pack sans
/// fenêtre (tests, automatisation). Le déroulé est écrit dans `<dossier>\blackat-pack.log`.
fn cli_backup(args: &[String]) -> Option<i32> {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let out = value("--backup")?;
    let out_dir = PathBuf::from(out);
    let _ = std::fs::create_dir_all(&out_dir);
    let log = std::fs::File::create(out_dir.join("blackat-pack.log")).ok()?;
    let progress = LogProgress(std::sync::Mutex::new(log), &pack::backup::STEPS);
    let opts = pack::backup::Options {
        out_dir,
        password: value("--password").unwrap_or_default(),
        force: args.iter().any(|a| a == "--force"),
    };
    let result = pack::backup::run(&opts, &progress);
    use std::io::Write;
    let mut f = progress.0.lock().unwrap();
    match result {
        Ok(outcome) => {
            let _ = writeln!(f, "PACK : {}", outcome.pack.display());
            for w in &outcome.manifest.warnings {
                let _ = writeln!(f, "ATTENTION : {w}");
            }
            Some(0)
        }
        Err(e) => {
            let _ = writeln!(f, "ERREUR : {e}");
            Some(1)
        }
    }
}

/// `blackat-launcher.exe --restore <pack extrait> --password <mdp> --sandbox <dossier>` :
/// restauration dans un PC simulé (tests). Volontairement **uniquement** en bac à sable :
/// la vraie installation passe par la fenêtre. Journal : `<dossier>lackat-restore.log`.
fn cli_restore(args: &[String]) -> Option<i32> {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let pack_dir = PathBuf::from(value("--restore")?);
    let Some(root) = value("--sandbox").map(PathBuf::from) else {
        eprintln!("--restore exige --sandbox <dossier> (la vraie installation se fait depuis la fenêtre)");
        return Some(2);
    };
    let _ = std::fs::create_dir_all(&root);
    let log = std::fs::File::create(root.join("blackat-restore.log")).ok()?;
    let progress = LogProgress(std::sync::Mutex::new(log), &pack::restore::STEPS);
    let opts = pack::restore::Options { pack_dir, password: value("--password").unwrap_or_default(), sandbox: Some(root) };
    let result = pack::restore::run(&opts, &progress);
    use std::io::Write;
    let mut f = progress.0.lock().unwrap();
    match result {
        Ok(o) => {
            for b in &o.backups {
                let _ = writeln!(f, "MIS DE CÔTÉ : {}", b.display());
            }
            for w in &o.warnings {
                let _ = writeln!(f, "ATTENTION : {w}");
            }
            for m in &o.missing {
                let _ = writeln!(f, "MANQUANT : {m}");
            }
            let _ = writeln!(f, "TERMINÉ");
            Some(0)
        }
        Err(e) => {
            let _ = writeln!(f, "ERREUR : {e}");
            Some(1)
        }
    }
}

/// Lance un programme console sans ouvrir de fenêtre.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Une appli à démarrer.
struct AppSpec {
    name: &'static str,
    exe: PathBuf,
    args: Vec<&'static str>,
    /// Nom du processus, pour savoir si l'appli tourne déjà.
    process: &'static str,
    /// Port du serveur WebSocket à attendre (None = on vérifie juste que le processus tourne).
    port: Option<u16>,
    hidden: bool,
    timeout: Duration,
}

fn env_path(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_default()
}

/// Port du serveur WebSocket d'OBS (Outils → Paramètres du serveur WebSocket).
fn obs_port() -> u16 {
    let path = env_path("APPDATA").join(r"obs-studio\plugin_config\obs-websocket\config.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["server_port"].as_u64())
        .map(|p| p as u16)
        .unwrap_or(4455)
}

/// Port du serveur WebSocket de Streamer.bot (Servers/Clients → WebSocket Server).
fn streamerbot_port(dir: &Path) -> u16 {
    std::fs::read_to_string(dir.join(r"data\settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["websockets"]["port"].as_u64())
        .map(|p| p as u16)
        .unwrap_or(8080)
}

fn specs() -> Vec<AppSpec> {
    let obs = env_path("ProgramFiles").join(r"obs-studio\bin\64bit\obs64.exe");
    let sb_dir = env_path("USERPROFILE").join(r"Documents\Streamer.bot");
    let here = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    vec![
        AppSpec {
            name: "OBS Studio",
            exe: obs,
            // Évite la fenêtre « OBS ne s'est pas fermé correctement / mode sans échec ».
            args: vec!["--disable-shutdown-check"],
            process: "obs64.exe",
            port: Some(obs_port()),
            hidden: false,
            timeout: Duration::from_secs(90),
        },
        AppSpec {
            name: "Streamer.bot",
            exe: sb_dir.join("Streamer.bot.exe"),
            args: vec![],
            process: "Streamer.bot.exe",
            port: Some(streamerbot_port(&sb_dir)),
            hidden: false,
            timeout: Duration::from_secs(90),
        },
        AppSpec {
            name: "Stream Deck",
            exe: here.join("soomfon-deck.exe"),
            args: vec![],
            process: "soomfon-deck.exe",
            port: None,
            hidden: true,
            timeout: Duration::from_secs(5),
        },
        AppSpec {
            name: "Éditeur Stream Deck",
            exe: here.join("soomfon-editor.exe"),
            args: vec![],
            process: "soomfon-editor.exe",
            port: None,
            hidden: false,
            timeout: Duration::from_secs(5),
        },
    ]
}

fn is_running(process: &str) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {process}"), "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&process.to_lowercase()))
        .unwrap_or(false)
}

fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Démarre une appli et attend qu'elle soit prête. Renvoie (succès, détail).
fn start(app: &AppSpec) -> (bool, String) {
    let already = is_running(app.process);
    // Préfixe affiché : on distingue une appli déjà ouverte d'une appli qu'on vient de démarrer.
    let how = if already { "déjà ouvert" } else { "démarré" };
    if !already {
        if !app.exe.exists() {
            return (false, "introuvable".into());
        }
        let mut cmd = Command::new(&app.exe);
        cmd.args(&app.args);
        // OBS et Streamer.bot cherchent leurs fichiers à côté de l'exécutable.
        if let Some(dir) = app.exe.parent() {
            cmd.current_dir(dir);
        }
        if app.hidden {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if let Err(e) = cmd.spawn() {
            return (false, format!("échec : {e}"));
        }
    }

    let deadline = Instant::now() + app.timeout;
    match app.port {
        Some(port) => {
            while Instant::now() < deadline {
                if port_open(port) {
                    return (true, format!("{how} · WebSocket {port}"));
                }
                std::thread::sleep(Duration::from_millis(400));
            }
            (false, format!("WebSocket {port} ne répond pas"))
        }
        None => {
            if !already {
                std::thread::sleep(Duration::from_millis(1500));
            }
            if is_running(app.process) { (true, how.into()) } else { (false, "s'est arrêté".into()) }
        }
    }
}

/// Flocons pseudo-aléatoires (générateur simple et déterministe).
fn flakes(count: usize) -> Vec<Flake> {
    let mut seed: u32 = 0x5EED_B1AC;
    let mut rnd = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    (0..count)
        .map(|_| {
            let depth = rnd(); // 0 = loin (petit, lent, pâle), 1 = près
            Flake {
                x: rnd(),
                speed: 0.05 + depth * 0.10,
                phase: rnd(),
                size: 1.5 + depth * 3.5,
                drift: 0.4 + rnd() * 1.2,
                opacity: 0.25 + depth * 0.6,
            }
        })
        .collect()
}

fn rows(states: &[(i32, String)], names: &[&str]) -> ModelRc<AppRow> {
    let rows: Vec<AppRow> = names
        .iter()
        .zip(states)
        .map(|(name, (state, detail))| AppRow { name: (*name).into(), state: *state, detail: detail.as_str().into() })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Dossier où créer le pack : le Bureau (ou Documents s'il n'existe pas).
fn backup_destination() -> PathBuf {
    // Pour les tests : BLACKAT_PACK_DIR remplace le Bureau.
    if let Some(dir) = std::env::var_os("BLACKAT_PACK_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    let desktop = home.join("Desktop");
    if desktop.is_dir() { desktop } else { home.join("Documents") }
}

/// Progression envoyée à la fenêtre depuis le thread de sauvegarde ou d'installation.
struct UiProgress {
    ui: slint::Weak<Launcher>,
    states: std::sync::Mutex<Vec<(i32, String)>>,
    steps: &'static [&'static str],
    show: fn(&Launcher, ModelRc<AppRow>),
}

impl UiProgress {
    fn new(ui: &Launcher, steps: &'static [&'static str], show: fn(&Launcher, ModelRc<AppRow>)) -> Self {
        let states = vec![(0, String::new()); steps.len()];
        show(ui, step_rows(steps, &states));
        Self { ui: ui.as_weak(), states: std::sync::Mutex::new(states), steps, show }
    }
}

impl pack::backup::Progress for UiProgress {
    fn step(&self, index: usize, state: i32, detail: &str) {
        let snapshot = {
            let mut s = self.states.lock().unwrap();
            s[index] = (state, detail.to_string());
            s.clone()
        };
        let (steps, show) = (self.steps, self.show);
        let _ = self.ui.upgrade_in_event_loop(move |ui| show(&ui, step_rows(steps, &snapshot)));
    }
}

fn step_rows(steps: &[&str], states: &[(i32, String)]) -> ModelRc<AppRow> {
    let rows: Vec<AppRow> = steps
        .iter()
        .zip(states)
        .map(|(name, (state, detail))| AppRow { name: (*name).into(), state: *state, detail: detail.as_str().into() })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Écran « Sauvegarder » : mot de passe, création du pack dans un thread, bilan.
fn install_backup(ui: &Launcher) {
    let weak = ui.as_weak();
    ui.on_open_backup(move || {
        let Some(ui) = weak.upgrade() else { return };
        ui.set_mode(1);
        ui.set_backup_started(false);
        ui.set_backup_done(false);
        ui.set_backup_failed(false);
        ui.set_backup_need_force(false);
        ui.set_backup_dest(backup_destination().to_string_lossy().to_string().into());
        ui.set_backup_message("".into());
    });

    let weak = ui.as_weak();
    ui.on_leave_backup(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_password("".into());
            ui.set_password2("".into());
            ui.set_mode(0);
        }
    });

    ui.on_open_pack_folder(|| {
        LAST_PACK.with(|p| {
            if let Some(p) = p.borrow().as_ref() {
                let _ = Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
            }
        });
    });

    let weak = ui.as_weak();
    ui.on_start_backup(move |force| {
        let Some(ui) = weak.upgrade() else { return };
        let (p1, p2) = (ui.get_password().to_string(), ui.get_password2().to_string());
        let fail = |msg: &str| {
            ui.set_backup_failed(true);
            ui.set_backup_message(msg.into());
        };
        if p1.chars().count() < 8 {
            return fail("Mot de passe trop court : 8 caractères minimum.");
        }
        if p1 != p2 {
            return fail("Les deux mots de passe ne sont pas identiques.");
        }
        if !force {
            let open: Vec<&str> = [("obs64.exe", "OBS"), ("Streamer.bot.exe", "Streamer.bot")]
                .into_iter()
                .filter(|(p, _)| pack::backup::is_running(p))
                .map(|(_, n)| n)
                .collect();
            if !open.is_empty() {
                ui.set_backup_need_force(true);
                return fail(&format!(
                    "{} {} ouvert : ferme-le pour une copie parfaite, ou sauvegarde quand même.",
                    open.join(" et "),
                    if open.len() > 1 { "sont" } else { "est" }
                ));
            }
        }

        ui.set_backup_failed(false);
        ui.set_backup_started(true);
        ui.set_backup_running(true);
        ui.set_backup_message("Création du pack…".into());
        let opts = pack::backup::Options { out_dir: backup_destination(), password: p1, force: true };
        ui.set_password("".into());
        ui.set_password2("".into());
        let progress = UiProgress::new(&ui, &pack::backup::STEPS, Launcher::set_backup_steps);
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let result = pack::backup::run(&opts, &progress);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_backup_running(false);
                match result {
                    Ok(outcome) => {
                        ui.set_backup_done(true);
                        let mut msg = format!("Pack créé : {}", outcome.pack.display());
                        if !outcome.manifest.warnings.is_empty() {
                            msg.push_str(&format!("\n{} avertissement(s) — voir manifest.json dans le pack.", outcome.manifest.warnings.len()));
                        }
                        ui.set_backup_message(msg.into());
                        LAST_PACK.with(|p| *p.borrow_mut() = Some(outcome.pack));
                    }
                    Err(e) => {
                        ui.set_backup_failed(true);
                        ui.set_backup_message(format!("Sauvegarde impossible : {e}").into());
                    }
                }
            });
        });
    });

}

/// Le launcher est-il lancé depuis un pack extrait (manifest.json + data.blackat à côté) ?
fn pack_here() -> Option<(PathBuf, pack::Manifest)> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    if !dir.join(pack::DATA_FILE).is_file() {
        return None;
    }
    let text = std::fs::read_to_string(dir.join(pack::MANIFEST_FILE)).ok()?;
    let manifest = serde_json::from_str(&text).ok()?;
    Some((dir, manifest))
}

/// Écran « Installer » : mot de passe, restauration dans un thread, bilan, lancement.
/// `BLACKAT_RESTORE_SANDBOX=<dossier>` (tests) restaure dans un PC simulé au lieu du vrai.
fn install_restore(ui: &Launcher, pack_dir: PathBuf, manifest: &pack::Manifest) {
    let date = manifest.created.split(' ').next().unwrap_or_default().to_string();
    let obs = manifest.obs_version.as_deref().map(|v| format!(" · OBS {v}")).unwrap_or_default();
    ui.set_install_info(format!("Pack du {date}{obs}").into());
    ui.set_mode(2);

    let weak = ui.as_weak();
    ui.on_reset_install(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_install_started(false);
            ui.set_install_failed(false);
            ui.set_install_message("".into());
        }
    });

    ui.on_launch_installed(|| {
        INSTALLED.with(|p| {
            if let Some(exe) = p.borrow().as_ref() {
                let _ = Command::new(exe).current_dir(exe.parent().unwrap_or(Path::new("."))).spawn();
                let _ = slint::quit_event_loop();
            }
        });
    });

    let weak = ui.as_weak();
    ui.on_start_install(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_install_running() {
            return;
        }
        let password = ui.get_password().to_string();
        if password.is_empty() {
            ui.set_install_failed(true);
            ui.set_install_message("Entre le mot de passe choisi lors de la sauvegarde.".into());
            return;
        }
        ui.set_password("".into());
        ui.set_install_failed(false);
        ui.set_install_done(false);
        ui.set_install_started(true);
        ui.set_install_running(true);
        ui.set_install_message("Installation en cours… (Windows peut demander l'autorisation pour installer OBS)".into());

        let sandbox = std::env::var_os("BLACKAT_RESTORE_SANDBOX").map(PathBuf::from);
        let opts = pack::restore::Options { pack_dir: pack_dir.clone(), password, sandbox };
        let progress = UiProgress::new(&ui, &pack::restore::STEPS, Launcher::set_install_steps);
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let result = pack::restore::run(&opts, &progress);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_install_running(false);
                match result {
                    Ok(o) => {
                        ui.set_install_done(true);
                        let mut notes: Vec<String> = o.warnings.clone();
                        if !o.missing.is_empty() {
                            notes.push(format!("{} fichier(s) des scènes introuvable(s), ex. {}", o.missing.len(), o.missing[0]));
                        }
                        let msg = if notes.is_empty() {
                            "Tout est installé. Le raccourci « Blackat » est sur le Bureau.".to_string()
                        } else {
                            let first: Vec<String> = notes.iter().take(3).cloned().collect();
                            format!("Installé, avec {} point(s) à vérifier :\n{}", notes.len(), first.join("\n"))
                        };
                        ui.set_install_message(msg.into());
                        INSTALLED.with(|p| *p.borrow_mut() = Some(o.launcher));
                    }
                    Err(e) => {
                        ui.set_install_failed(true);
                        ui.set_install_message(format!("Installation impossible : {e}").into());
                    }
                }
            });
        });
    });
}

thread_local! {
    /// Chemin du dernier pack créé (lu par « Ouvrir le dossier » ; thread de l'interface).
    static LAST_PACK: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    /// Launcher installé par « Installer » (lu par « Lancer Blackat »).
    static INSTALLED: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = cli_backup(&args).or_else(|| cli_restore(&args)) {
        std::process::exit(code);
    }
    // `--verify-pack <zip> --password <mdp> --report <fichier>` : contrôle d'un pack (tests).
    if let Some(i) = args.iter().position(|a| a == "--verify-pack") {
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|j| args.get(j + 1)).cloned().unwrap_or_default();
        let pack = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
        let tmp = std::env::temp_dir().join("blackat-verify.data");
        let result = pack::extract_data(&pack, &tmp).and_then(|()| pack::verify_data(&tmp, &value("--password")));
        let _ = std::fs::remove_file(&tmp);
        let text = match &result {
            Ok((n, b)) => format!("OK : {n} fichiers déchiffrés ({b} octets)"),
            Err(e) => format!("REFUS : {e}"),
        };
        let _ = std::fs::write(value("--report"), text);
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }

    let ui = Launcher::new()?;
    ui.set_flakes(ModelRc::new(VecModel::from(flakes(90))));

    let names: Vec<&'static str> = specs().iter().map(|s| s.name).collect();
    ui.set_apps(rows(&vec![(0, String::new()); names.len()], &names));

    // Déplacer la fenêtre sans bordure en la faisant glisser.
    let weak = ui.as_weak();
    ui.on_drag(move |dx, dy| {
        if let Some(ui) = weak.upgrade() {
            let w = ui.window();
            let s = w.scale_factor();
            let p = w.position();
            w.set_position(PhysicalPosition::new(p.x + (dx * s) as i32, p.y + (dy * s) as i32));
        }
    });

    install_backup(&ui);
    if let Some((dir, manifest)) = pack_here() {
        install_restore(&ui, dir, &manifest);
        // Aide au test, uniquement en PC simulé : BLACKAT_TEST_PASSWORD lance l'installation.
        if std::env::var_os("BLACKAT_RESTORE_SANDBOX").is_some() {
            if let Ok(pw) = std::env::var("BLACKAT_TEST_PASSWORD") {
                ui.set_password(pw.as_str().into());
                ui.invoke_start_install();
            }
        }
    }

    // Aide au test : BLACKAT_LAUNCHER_SCREEN=backup ouvre l'écran Sauvegarde ;
    // BLACKAT_TEST_PASSWORD le remplit et lance la sauvegarde.
    if std::env::var("BLACKAT_LAUNCHER_SCREEN").as_deref() == Ok("backup") {
        ui.invoke_open_backup();
        if let Ok(pw) = std::env::var("BLACKAT_TEST_PASSWORD") {
            ui.set_password(pw.as_str().into());
            ui.set_password2(pw.as_str().into());
            ui.invoke_start_backup(true);
        }
    }

    ui.on_close_window(|| {
        let _ = slint::quit_event_loop();
    });

    // Aide au développement : BLACKAT_SNAPSHOT=<fichier.png> capture la fenêtre après 2 s puis ferme.
    if let Ok(out) = std::env::var("BLACKAT_SNAPSHOT") {
        let weak = ui.as_weak();
        Timer::single_shot(Duration::from_millis(2000), move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Ok(buf) = ui.window().take_snapshot() {
                if let Some(img) = image::RgbaImage::from_raw(buf.width(), buf.height(), buf.as_bytes().to_vec()) {
                    let _ = img.save(&out);
                }
            }
            let _ = slint::quit_event_loop();
        });
    }

    let weak = ui.as_weak();
    ui.on_launch(move || {
        let Some(ui) = weak.upgrade() else { return };
        ui.set_busy(true);
        ui.set_failed(false);
        ui.set_message("Démarrage en cours…".into());

        let weak = weak.clone();
        let names = names.clone();
        // Les attentes se font hors du thread de l'interface (la neige continue de tomber).
        std::thread::spawn(move || {
            let apps = specs();
            let mut states: Vec<(i32, String)> = vec![(0, "en attente".into()); apps.len()];
            let push = |states: &Vec<(i32, String)>| {
                let states = states.clone();
                let names = names.clone();
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_apps(rows(&states, &names)));
            };
            push(&states);

            let mut all_ok = true;
            for (i, app) in apps.iter().enumerate() {
                states[i] = (1, "démarrage…".into());
                push(&states);
                let (ok, detail) = start(app);
                all_ok &= ok;
                states[i] = (if ok { 2 } else { 3 }, detail);
                push(&states);
            }

            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                if all_ok {
                    ui.set_done(true);
                    ui.set_message("Tout est lancé. La fenêtre va se fermer.".into());
                    Timer::single_shot(Duration::from_millis(2500), || {
                        let _ = slint::quit_event_loop();
                    });
                } else {
                    ui.set_failed(true);
                    ui.set_message("Une appli n'a pas démarré (voir ci-dessus). Corrige puis clique « Réessayer ».".into());
                }
            });
        });
    });

    ui.run()
}
