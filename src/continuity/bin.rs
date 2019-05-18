use clap::{self, crate_name, crate_version, value_t, App, Arg, ArgGroup, ArgMatches};
use log::*;
use rand::distributions::{Distribution, Uniform};
use std::fs::File;
use std::net::TcpListener;
use std::path::Path;
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::Duration;
use stderrlog;
use torrent::choking::Choke;
use torrent::connection::{ConnInfo, Connection};
use torrent::metainfo::Metainfo;
use torrent::selection::{Bitos, Inorder, Rare};
use torrent::storage::PieceStore;
use torrent::tracker::http::HTTP;
use torrent::tracker::{Discover, TorrentState};

fn setup() -> ArgMatches<'static> {
    App::new(crate_name!())
        .version(crate_version!())
        .arg(
            Arg::with_name("seed")
                .short("s")
                .long("seed")
                .help("Enable seeding after download completion"),
        )
        .arg(
            Arg::with_name("file")
                .short("f")
                .long("file")
                .takes_value(true)
                .value_name("FILE")
                .validator(|name| {
                    match File::open(Path::new(&name)) {
                        Err(e) => return Err(format!("{}", e)),
                        Ok(_) => {}
                    }

                    Ok(())
                })
                .help("File used to bypass download phase"),
        )
        .group(ArgGroup::with_name("seedmode").args(&["seed", "file"]))
        .arg(
            Arg::with_name("torrent")
                .takes_value(true)
                .multiple(false)
                .value_name("TORRENT FILE")
                .required(true),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .takes_value(true)
                .multiple(false)
                .value_name("PORT")
                .default_value("8888")
                .help("Port to listen for new connections"),
        )
        .arg(
            Arg::with_name("selector")
                .short("a")
                .long("selector")
                .takes_value(true)
                .multiple(false)
                .value_name("ALGORITHM")
                .default_value("inorder")
                .possible_values(&["inorder", "rarest", "bitos"])
                .help("Piece Selection strategy to use"),
        )
        .arg(
            Arg::with_name("logged_modules")
                .short("m")
                .long("module")
                .takes_value(true)
                .multiple(true)
                .value_name("MODULE"),
        )
        .arg(
            Arg::with_name("verbosity")
                .short("v")
                .multiple(true)
                .help("Increase message verbosity"),
        )
        .get_matches()
}

enum Event {
    Conn(Connection),
}

fn make_id() -> String {
    let mut rng = rand::thread_rng();
    let num_gen = Uniform::new('1' as u8, '9' as u8);
    format!(
        "-CN0010-{}",
        num_gen
            .sample_iter(&mut rng)
            .take(12)
            .map(|x| x as char)
            .collect::<String>()
    )
}

struct Listener {
    conn: TcpListener,
    tx: mpsc::Sender<Event>,
    metainfo: Arc<Metainfo>,
    client_id: Arc<String>,
    store: Arc<RwLock<PieceStore>>,
}

impl Listener {
    fn start(self) -> Result<(), failure::Error> {
        for stream in self.conn.incoming() {
            if let Ok(stream) = stream {
                debug!("New connection: {}", stream.peer_addr().unwrap());
                let id = Arc::new(stream.peer_addr().unwrap().to_string());
                match self.tx.send(Event::Conn(
                    match Connection::new(
                        stream,
                        ConnInfo {
                            store: self.store.clone(),
                            metainfo: self.metainfo.clone(),
                            reader_buffer_len: None,
                            writer_buffer_len: None,
                            client_id: self.client_id.clone(),
                            id,
                        },
                    ) {
                        Ok(c) => c,
                        Err(e) => {
                            error!("connection error: {}", e);
                            continue;
                        }
                    },
                )) {
                    Err(_) => return Ok(()),
                    _ => continue,
                }
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), failure::Error> {
    let matches = setup();
    stderrlog::new()
        .module(module_path!())
        .modules(matches.values_of("logged_modules").unwrap_or_default())
        .verbosity(matches.occurrences_of("verbosity") as usize)
        .init()
        .unwrap();

    // Parse metainfo
    let metainfo =
        Arc::new(value_t!(matches.value_of("torrent"), Metainfo).unwrap_or_else(|e| e.exit()));
    debug!("Parsed metainfo for {}", metainfo.info.name);

    // Piece Selector
    let store;
    match matches.value_of("selector").unwrap() {
        "inorder" => {
            store = Arc::new(RwLock::new(PieceStore::new(
                &metainfo,
                Box::new(Inorder::default()),
            )))
        }
        "rarest" => {
            store = Arc::new(RwLock::new(PieceStore::new(
                &metainfo,
                Box::new(Rare::default()),
            )))
        }
        "bitos" => {
            store = Arc::new(RwLock::new(PieceStore::new(
                &metainfo,
                Box::new(Bitos::default()),
            )))
        }
        s => {
            clap::Error::with_description(
                &format!("{} is an invalid piece selection strategy", s),
                clap::ErrorKind::InvalidValue,
            )
            .exit();
        }
    };

    // Bootstrap file
    match matches.value_of("file") {
        Some(f) => {
            debug!("Bootstrap from {}", f);
            store.write().unwrap().bootstrap(&metainfo, f)?;
        }
        None => {}
    }

    let (tx, rx) = mpsc::channel::<Event>();
    let client_id = Arc::new(make_id());
    info!("Client ID: {}", &client_id);
    let port = value_t!(matches.value_of("port"), u16).unwrap_or_else(|e| e.exit());
    let listener = Listener {
        conn: TcpListener::bind(format!("0.0.0.0:{}", port))?,
        tx: tx.clone(),
        metainfo: metainfo.clone(),
        store: store.clone(),
        client_id: client_id.clone(),
    };
    let listen_addr = listener.conn.local_addr().unwrap();
    let _listener_handle = thread::spawn(move || listener.start());
    info!("Listener started on {}", listen_addr);

    // Start metric server
    // let _metric_handle = thread::spawn(move || loop {
    //     unimplemented!()
    // });

    // Announce to tracker
    let c = reqwest::Client::new();
    let mut http = HTTP::new(metainfo.clone(), client_id.clone(), port, &c);
    let peers = http.get_peers(
        &TorrentState {
            uploaded: 0,
            downloaded: 0,
            left: metainfo.info.length as u64,
        },
        None,
    )?;
    info!("Got {} peers from tracker", peers.len() - 1); // One of the peers is always self

    let mut limiter = ratelimit::Builder::new()
        .capacity(1)
        .quantum(1)
        .interval(Duration::from_secs(10))
        .build();
    let mut choker = Choke::new();
    let mut optimistic_unchoke_counter = 0;

    // Connect to available peers
    for peer in peers.into_iter() {
        let conn = match Connection::connect(
            &peer.addr,
            ConnInfo {
                store: store.clone(),
                metainfo: metainfo.clone(),
                reader_buffer_len: None,
                writer_buffer_len: None,
                client_id: client_id.clone(),
                id: Arc::new(peer.addr.to_string()),
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                warn!("{}", e);
                continue;
            }
        };
        debug!("New connection: {}", peer.addr);
        choker.add(conn)
    }

    // Download Loop
    // Rate limited loop with alternate channel trigger
    while { store.read().unwrap().left != 0 } {
        debug!("Download loop");
        // Rate limited loop
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Conn(conn) => choker.add(conn),
            }
        }

        if optimistic_unchoke_counter == 0 {
            debug!("Optimistic Unchoke");
            optimistic_unchoke_counter = 3;
            choker.download(true);
        } else {
            choker.download(false);
        }

        limiter.wait();
    }

    // Seed loop
    // Change choking metrics to use download rate rather than upload
    if matches.is_present("file") || matches.is_present("seed") {
        optimistic_unchoke_counter = 0;
        loop {
            // Rate limited loop
            debug!("Seed loop");
            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::Conn(conn) => choker.add(conn),
                }
            }

            if optimistic_unchoke_counter == 0 {
                debug!("Optimistic Unchoke");
                optimistic_unchoke_counter = 3;
                choker.upload(true);
            } else {
                choker.upload(false);
            }

            limiter.wait();
        }
    }

    Ok(())
}
