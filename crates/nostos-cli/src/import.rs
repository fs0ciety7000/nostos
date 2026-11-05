//! `nostos import <folder> --trip <slug>`: sends a folder of photos to a
//! Nostos API.
//!
//! 1. Reads every photo locally first (nostos-exif) and reports what it found:
//!    how many have GPS, which film simulations, which files are unreadable.
//! 2. Creates the trip if `--title` is given and it does not exist yet.
//! 3. Skips files the trip already has (same name and size).
//! 4. Uploads with a few parallel workers: reserve, PUT the file where the API
//!    says (straight to R2 in production), confirm. Each step is retried on
//!    network errors and 5xx responses.
//! 5. With `--wait`, follows the processing and prints the stops found.
//!
//! The owner token is read from `NOSTOS_OWNER_TOKEN` only, never from the
//! command line (where it would end up in shell history and `ps`).

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const USAGE: &str = "usage: nostos import <folder> --trip <slug> [--title <title>] [--jobs <n>] [--dry-run] [--wait]

Sends the .jpg/.jpeg/.raf files under <folder> to a Nostos API.

  --trip <slug>     trip to add the photos to, e.g. quebec-2026
  --title <title>   create the trip with this title if it does not exist
  --jobs <n>        parallel uploads (default 4, max 16)
  --dry-run         read the photos and show what would be sent, send nothing
  --wait            wait for the API to process the photos, then list the stops

environment:
  NOSTOS_API_URL      API base URL (default http://127.0.0.1:8080)
  NOSTOS_OWNER_TOKEN  owner token of the instance (required unless --dry-run)";

#[derive(Debug)]
struct Opts {
    folder: PathBuf,
    trip: String,
    title: Option<String>,
    jobs: usize,
    dry_run: bool,
    wait: bool,
}

fn parse(args: Vec<String>) -> Result<Opts, String> {
    let mut it = args.into_iter();
    let (mut folder, mut trip, mut title, mut jobs, mut dry_run, mut wait) =
        (None, None, None, 4, false, false);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--trip" => trip = it.next(),
            "--title" => title = it.next(),
            "--jobs" => {
                jobs = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--jobs needs a number")?;
            }
            "--dry-run" => dry_run = true,
            "--wait" => wait = true,
            _ if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ if folder.is_none() => folder = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument {a}")),
        }
    }
    let folder = folder.ok_or("missing <folder>")?;
    let trip = trip.ok_or("missing --trip <slug>")?;
    Ok(Opts {
        folder,
        trip,
        title,
        jobs: jobs.clamp(1, 16),
        dry_run,
        wait,
    })
}

pub fn run(args: Vec<String>) -> ExitCode {
    let opts = match parse(args) {
        Ok(o) => o,
        Err(e) if e.is_empty() => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match import(&opts) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

// ---------- local reading ----------

struct LocalPhoto {
    path: PathBuf,
    name: String,
    size: u64,
}

struct Survey {
    photos: Vec<LocalPhoto>,
    with_gps: usize,
    films: Vec<(String, usize)>,
    unreadable: Vec<(PathBuf, String)>,
}

fn survey(folder: &Path) -> Result<Survey, String> {
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let mut paths = Vec::new();
    crate::scan::walk(folder, &mut paths);
    paths.sort();
    let mut photos = Vec::new();
    let mut with_gps = 0;
    let mut films: Vec<(String, usize)> = Vec::new();
    let mut unreadable = Vec::new();
    for path in paths {
        match nostos_exif::read_path(&path) {
            Ok(m) => {
                with_gps += usize::from(m.gps.is_some());
                let film = m
                    .fuji
                    .and_then(|f| f.film_simulation)
                    .map_or_else(|| "(no film simulation)".into(), |f| f.name());
                match films.iter_mut().find(|(f, _)| *f == film) {
                    Some((_, n)) => *n += 1,
                    None => films.push((film, 1)),
                }
                let size = std::fs::metadata(&path).map_or(0, |m| m.len());
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                photos.push(LocalPhoto { path, name, size });
            }
            Err(e) => unreadable.push((path, e.to_string())),
        }
    }
    films.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(Survey {
        photos,
        with_gps,
        films,
        unreadable,
    })
}

/// "24.3 MB"
fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.0} kB", (b / 1e3).max(1.0))
    }
}

// ---------- API ----------

struct Api {
    /// Honours `HTTPS_PROXY` / `HTTP_PROXY` for remote hosts.
    agent: ureq::Agent,
    /// No proxy, for localhost.
    direct: ureq::Agent,
    base: String,
    token: String,
}

#[derive(Debug)]
enum CallError {
    /// The API answered with an error status and (usually) a message.
    Status(u16, String),
    /// No answer: network, TLS, timeout.
    Transport(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status(code, msg) if msg.is_empty() => write!(f, "HTTP {code}"),
            Self::Status(code, msg) => write!(f, "HTTP {code}: {msg}"),
            Self::Transport(e) => write!(f, "{e}"),
        }
    }
}

impl CallError {
    fn retryable(&self) -> bool {
        matches!(self, Self::Transport(_))
            || matches!(self, Self::Status(code, _) if *code >= 500 || *code == 429)
    }
}

fn convert(r: Result<ureq::Response, ureq::Error>) -> Result<ureq::Response, CallError> {
    match r {
        Ok(res) => Ok(res),
        Err(ureq::Error::Status(code, res)) => {
            let body: Value = res.into_json().unwrap_or(Value::Null);
            let msg = body["message"].as_str().unwrap_or_default().to_string();
            Err(CallError::Status(code, msg))
        }
        Err(ureq::Error::Transport(t)) => Err(CallError::Transport(t.to_string())),
    }
}

/// Runs `f` up to 4 times, backing off 1 s, 3 s, 9 s on retryable errors.
fn with_retry<T>(mut f: impl FnMut() -> Result<T, CallError>) -> Result<T, CallError> {
    let mut delay = Duration::from_secs(1);
    for attempt in 1..=4 {
        match f() {
            Err(e) if e.retryable() && attempt < 4 => {
                std::thread::sleep(delay);
                delay *= 3;
            }
            other => return other,
        }
    }
    unreachable!("the last attempt returns")
}

/// True for localhost and loopback addresses, which must never go through a
/// proxy (ureq's proxy-from-env does not read `NO_PROXY`).
fn is_loopback(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = host.strip_prefix('[') {
        v6.split(']').next().unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or_default()
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

impl Api {
    fn new(base: &str, token: String) -> Self {
        let builder = || {
            ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(15))
                .timeout_read(Duration::from_secs(120))
                .timeout_write(Duration::from_secs(600))
                .user_agent(concat!("nostos-cli/", env!("CARGO_PKG_VERSION")))
        };
        Self {
            agent: builder().try_proxy_from_env(true).build(),
            direct: builder().try_proxy_from_env(false).build(),
            base: base.trim_end_matches('/').to_string(),
            token,
        }
    }

    fn agent_for(&self, url: &str) -> &ureq::Agent {
        if is_loopback(url) {
            &self.direct
        } else {
            &self.agent
        }
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        let url = format!("{}{path}", self.base);
        self.agent_for(&url)
            .request(method, &url)
            .set("Authorization", &format!("Bearer {}", self.token))
    }

    fn get(&self, path: &str) -> Result<Value, CallError> {
        with_retry(|| {
            convert(self.request("GET", path).call())?
                .into_json()
                .map_err(|e| CallError::Transport(e.to_string()))
        })
    }

    fn post(&self, path: &str, body: &Value) -> Result<Value, CallError> {
        with_retry(|| {
            convert(self.request("POST", path).send_json(body.clone()))?
                .into_json()
                .map_err(|e| CallError::Transport(e.to_string()))
        })
    }

    /// Reserves, uploads and confirms one photo.
    fn send(&self, trip: &str, p: &LocalPhoto) -> Result<(), CallError> {
        let res = self.post(
            &format!("/trips/{trip}/uploads"),
            &json!({ "file_name": p.name, "byte_size": p.size }),
        )?;
        let id = res["photo_id"]
            .as_str()
            .ok_or_else(|| CallError::Transport("no photo_id in the API answer".into()))?
            .to_string();
        let up = &res["upload"];
        let url = up["url"].as_str().unwrap_or_default().to_string();
        let method = up["method"].as_str().unwrap_or("PUT").to_string();
        let needs_token = up["requires_owner_token"].as_bool().unwrap_or(false);
        with_retry(|| {
            let file = File::open(&p.path).map_err(|e| {
                CallError::Transport(format!("cannot read {}: {e}", p.path.display()))
            })?;
            let mut req = self
                .agent_for(&url)
                .request(&method, &url)
                .set("Content-Length", &p.size.to_string())
                .set("Content-Type", "application/octet-stream");
            if needs_token {
                req = req.set("Authorization", &format!("Bearer {}", self.token));
            }
            convert(req.send(file)).map(|_| ())
        })?;
        self.post(&format!("/photos/{id}/complete"), &json!({}))
            .map(|_| ())
    }
}

/// Files already in the trip (by name and size), except failed ones, which are sent again.
fn already_sent(files: &Value) -> HashSet<(String, u64)> {
    files
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["status"].as_str() != Some("failed"))
        .filter_map(|f| {
            Some((
                f["file_name"].as_str()?.to_string(),
                f["byte_size"].as_u64()?,
            ))
        })
        .collect()
}

#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // one linear flow, read top to bottom
fn import(o: &Opts) -> Result<bool, String> {
    let s = survey(&o.folder)?;
    let total: u64 = s.photos.iter().map(|p| p.size).sum();
    println!(
        "{} photos ({}) in {} · {} with GPS",
        s.photos.len(),
        human(total),
        o.folder.display(),
        s.with_gps
    );
    for (film, n) in &s.films {
        println!("  {n:>5}  {film}");
    }
    for (p, e) in &s.unreadable {
        println!("  skipped {}: {e}", p.display());
    }
    if s.photos.is_empty() {
        return Err("no readable .jpg or .raf file in this folder".into());
    }

    let base = std::env::var("NOSTOS_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let token = std::env::var("NOSTOS_OWNER_TOKEN").unwrap_or_default();
    if token.is_empty() && !o.dry_run {
        return Err(
            "NOSTOS_OWNER_TOKEN is not set: export the owner token of your instance first".into(),
        );
    }
    let api = Api::new(&base, token);

    let existing = if api.token.is_empty() {
        HashSet::new()
    } else {
        match api.get(&format!("/trips/{}", o.trip)) {
            Ok(_) => already_sent(
                &api.get(&format!("/trips/{}/files", o.trip))
                    .map_err(|e| format!("listing the trip's files: {e}"))?,
            ),
            Err(CallError::Status(404, _)) => match (&o.title, o.dry_run) {
                (Some(_), true) => HashSet::new(),
                (Some(title), false) => {
                    api.post("/trips", &json!({ "slug": o.trip, "title": title }))
                        .map_err(|e| format!("creating trip {}: {e}", o.trip))?;
                    println!("trip {} created", o.trip);
                    HashSet::new()
                }
                (None, _) => {
                    return Err(format!(
                        "trip {} does not exist on {base}: add --title \"…\" to create it",
                        o.trip
                    ))
                }
            },
            Err(CallError::Status(401, _)) => {
                return Err(format!("{base} refused the owner token (HTTP 401)"))
            }
            Err(e) => return Err(format!("reaching {base}: {e}")),
        }
    };

    let local = s.photos.len();
    let todo: Vec<LocalPhoto> = s
        .photos
        .into_iter()
        .filter(|p| !existing.contains(&(p.name.clone(), p.size)))
        .collect();
    if todo.len() < local {
        println!(
            "{} already in {}, {} to send",
            local - todo.len(),
            o.trip,
            todo.len()
        );
    }
    if o.dry_run {
        println!("dry run: nothing sent");
        return Ok(true);
    }
    if todo.is_empty() {
        println!("nothing to send");
        return Ok(true);
    }

    let n = todo.len();
    let queue = Arc::new(Mutex::new(
        todo.into_iter().enumerate().collect::<VecDeque<_>>(),
    ));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let api = Arc::new(api);
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..o.jobs.min(n) {
            let (queue, failures, api) = (queue.clone(), failures.clone(), api.clone());
            scope.spawn(move || loop {
                let Some((i, p)) = queue.lock().expect("queue").pop_front() else {
                    break;
                };
                match api.send(&o.trip, &p) {
                    Ok(()) => println!(
                        "[{:>width$}/{n}] {}  {}",
                        i + 1,
                        p.name,
                        human(p.size),
                        width = n.to_string().len()
                    ),
                    Err(e) => {
                        println!(
                            "[{:>width$}/{n}] {}  FAILED: {e}",
                            i + 1,
                            p.name,
                            width = n.to_string().len()
                        );
                        failures
                            .lock()
                            .expect("failures")
                            .push((p.name, e.to_string()));
                    }
                }
            });
        }
    });
    let failed = failures.lock().expect("failures").len();
    println!(
        "{} sent, {failed} failed, in {:.0} s",
        n - failed,
        started.elapsed().as_secs_f64()
    );

    if o.wait && failed < n {
        wait_for_processing(&api, &o.trip)?;
    }
    if failed > 0 {
        println!("run the same command again to retry the failed files");
    }
    Ok(failed == 0)
}

/// Whether the processing of a trip is over, from `GET /trips/{slug}`.
///
/// Stops are recomputed a moment after the last photo is processed, so with
/// photos that have GPS we wait for them to appear, but at most
/// `STOPS_GRACE` after nothing is in flight: a trip without any GPS photo
/// never gets stops and must not keep the command waiting.
fn settled(trip: &Value, idle_for: Duration) -> bool {
    const STOPS_GRACE: Duration = Duration::from_secs(20);
    let photos = &trip["photos"];
    if photos["in_flight"].as_u64().unwrap_or(0) > 0 {
        return false;
    }
    let has_stops = trip["stops"].as_array().is_some_and(|s| !s.is_empty());
    let with_gps = photos["with_gps"].as_u64().unwrap_or(0);
    has_stops || with_gps == 0 || idle_for >= STOPS_GRACE
}

fn wait_for_processing(api: &Api, trip: &str) -> Result<(), String> {
    let started = Instant::now();
    let mut idle_since: Option<Instant> = None;
    loop {
        let t = api
            .get(&format!("/trips/{trip}"))
            .map_err(|e| format!("following the processing: {e}"))?;
        let in_flight = t["photos"]["in_flight"].as_u64().unwrap_or(0);
        let idle_for = if in_flight == 0 {
            idle_since.get_or_insert_with(Instant::now).elapsed()
        } else {
            idle_since = None;
            Duration::ZERO
        };
        if settled(&t, idle_for) || started.elapsed() > Duration::from_secs(1800) {
            let stops = t["stops"].as_array().cloned().unwrap_or_default();
            println!(
                "processed: {} ready, {} failed · {} stops · {} km",
                t["photos"]["ready"],
                t["photos"]["failed"],
                stops.len(),
                t["route_km"]
                    .as_f64()
                    .map_or_else(|| "?".into(), |k| format!("{k:.0}"))
            );
            for s in stops.iter().filter(|s| s["is_waypoint"] != true) {
                println!(
                    "  {:>2}. {:<28} {} photos  {}",
                    s["seq"],
                    s["name"].as_str().unwrap_or("(unnamed)"),
                    s["photos"],
                    s["film_simulation"].as_str().unwrap_or("")
                );
            }
            if t["photos"]["with_gps"].as_u64() == Some(0) {
                println!(
                    "no photo of this trip has a GPS position, so no stop can be placed on the globe yet.\n\
                     Fujifilm bodies only record GPS when the camera is paired with the Fujifilm app \
                     (location sync). The photos are stored; they will be placed once a GPS track \
                     can be matched to them or they are placed by hand."
                );
            }
            return Ok(());
        }
        if in_flight == 0 {
            println!("  waiting for the stops…");
        } else {
            println!("  {in_flight} still processing…");
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options() {
        let o = parse(
            [
                "photos",
                "--trip",
                "quebec-2026",
                "--title",
                "L'été indien",
                "--jobs",
                "99",
                "--wait",
            ]
            .map(String::from)
            .to_vec(),
        )
        .unwrap();
        assert_eq!(
            (
                o.trip.as_str(),
                o.title.as_deref(),
                o.jobs,
                o.wait,
                o.dry_run
            ),
            ("quebec-2026", Some("L'été indien"), 16, true, false)
        );
        assert!(parse(vec!["photos".into()]).unwrap_err().contains("--trip"));
        assert!(parse(vec!["--trip".into(), "x".into()])
            .unwrap_err()
            .contains("folder"));
        assert!(parse(vec![
            "a".into(),
            "--trip".into(),
            "x".into(),
            "--token".into()
        ])
        .unwrap_err()
        .contains("unknown option"));
    }

    #[test]
    fn skips_sent_files_but_resends_failed_ones() {
        let files = json!([
            { "file_name": "A.JPG", "byte_size": 10, "status": "ready" },
            { "file_name": "B.RAF", "byte_size": 20, "status": "failed" },
            { "file_name": "C.JPG", "byte_size": 30, "status": "pending" }
        ]);
        let s = already_sent(&files);
        assert!(s.contains(&("A.JPG".into(), 10)) && s.contains(&("C.JPG".into(), 30)));
        assert!(!s.contains(&("B.RAF".into(), 20)));
        assert!(
            !s.contains(&("A.JPG".into(), 11)),
            "same name, other size is another file"
        );
    }

    #[test]
    fn retries_only_what_can_succeed_later() {
        assert!(CallError::Transport("reset".into()).retryable());
        assert!(CallError::Status(503, String::new()).retryable());
        assert!(CallError::Status(429, String::new()).retryable());
        assert!(!CallError::Status(400, "bad".into()).retryable());
        assert!(!CallError::Status(401, String::new()).retryable());
        let mut calls = 0;
        let r: Result<(), CallError> = with_retry(|| {
            calls += 1;
            Err(CallError::Status(409, "conflict".into()))
        });
        assert!(r.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn loopback_hosts_bypass_the_proxy() {
        for u in [
            "http://127.0.0.1:8080",
            "http://localhost/x",
            "http://[::1]:8080/a",
            "http://user@127.0.0.2:1",
        ] {
            assert!(is_loopback(u), "{u}");
        }
        for u in [
            "https://nostos.fs0ciety.org",
            "https://abc.r2.cloudflarestorage.com/b/k?X=1",
            "http://10.0.0.5:8080",
        ] {
            assert!(!is_loopback(u), "{u}");
        }
    }

    #[test]
    fn waiting_ends_even_without_gps() {
        let trip = |in_flight: u64, with_gps: u64, stops: usize| json!({ "photos": { "in_flight": in_flight, "with_gps": with_gps }, "stops": vec![json!({}); stops] });
        let none = Duration::ZERO;
        assert!(!settled(&trip(2, 5, 0), none), "photos still processing");
        assert!(
            settled(&trip(0, 0, 0), none),
            "no GPS: no stop will ever come"
        );
        assert!(
            !settled(&trip(0, 5, 0), none),
            "GPS photos: stops are on their way"
        );
        assert!(settled(&trip(0, 5, 3), none));
        assert!(
            settled(&trip(0, 5, 0), Duration::from_secs(25)),
            "do not wait forever for stops"
        );
    }

    #[test]
    fn sizes() {
        assert_eq!(human(512), "1 kB");
        assert_eq!(human(24_300_000), "24.3 MB");
        assert_eq!(human(1_500_000_000), "1.5 GB");
    }
}
